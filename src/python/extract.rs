//! 安全解压：`.tar.gz` 去一层顶目录（对齐官方 install.sh 的
//! `--strip-components 1`），`.zip` 平铺解压。
//!
//! 所有条目做 confinement：绝对路径、`..` 逃逸一律拒绝；
//! 符号链接仅当目标仍落在目标目录内才创建（pbs 的 unix 布局需要它们）。

use std::fs;
use std::path::{Component, Path, PathBuf};

/// 词法归一化（与 config.rs 同规则：不碰磁盘解析符号链接）。
fn normalize_lexically(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            _ => out.push(c.as_os_str()),
        }
    }
    out
}

/// 把归档内相对路径映射到目标目录下；逃逸时返回 `Err`。
fn confine(dest: &Path, entry: &Path) -> Result<PathBuf, String> {
    if entry.is_absolute() {
        return Err(format!("归档条目为绝对路径：{}", entry.display()));
    }
    let joined = normalize_lexically(&dest.join(entry));
    if !joined.starts_with(normalize_lexically(dest)) {
        return Err(format!("归档条目逃逸目标目录：{}", entry.display()));
    }
    Ok(joined)
}

/// 解压归档 `archive` 到 `dest`（已存在则合并覆盖）。
pub fn extract(archive: &Path, dest: &Path) -> Result<(), String> {
    let name = archive.to_string_lossy().to_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, dest)
    } else if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else {
        Err(format!("不支持的归档格式：{}", archive.display()))
    }
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| format!("无法打开归档：{e}"))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut ar = tar::Archive::new(gz);
    let entries = ar.entries().map_err(|e| format!("无法读取归档：{e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("归档条目损坏：{e}"))?;
        let entry_path = entry.path().map_err(|e| format!("归档路径非法：{e}"))?;
        let Some(stripped) = strip_first_components(&entry_path, 1) else {
            continue;
        };
        let out = confine(dest, &stripped)?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("无法创建目录：{e}"))?;
        } else if kind.is_symlink() || kind.is_hard_link() {
            let target: PathBuf = entry
                .link_name()
                .map_err(|e| format!("链接目标非法：{e}"))?
                .ok_or_else(|| "链接缺目标".to_string())?
                .into_owned();
            if target.is_absolute() {
                return Err(format!("链接目标为绝对路径：{}", target.display()));
            }
            let resolved = normalize_lexically(&out.parent().unwrap_or(dest).join(&target));
            if !resolved.starts_with(normalize_lexically(dest)) {
                return Err(format!("链接目标逃逸：{}", target.display()));
            }
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("无法创建目录：{e}"))?;
            }
            let _ = fs::remove_file(&out);
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &out).map_err(|e| format!("无法创建链接：{e}"))?;
            #[cfg(not(unix))]
            return Err(format!("该平台不支持归档中的链接：{}", target.display()));
        } else if kind.is_file() {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("无法创建目录：{e}"))?;
            }
            entry
                .unpack(&out)
                .map_err(|e| format!("无法解压条目：{e}"))?;
        }
    }
    Ok(())
}

/// 去掉前 `n` 层普通段；不足 `n` 层、或首段非普通段时返回 `None`。
fn strip_first_components(entry: &Path, n: usize) -> Option<PathBuf> {
    let mut comps = entry.components();
    for _ in 0..n {
        match comps.next() {
            Some(Component::Normal(_)) => {}
            _ => return None,
        }
    }
    let rest: PathBuf = comps.collect();
    if rest.as_os_str().is_empty() {
        None
    } else {
        Some(rest)
    }
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| format!("无法打开归档：{e}"))?;
    let mut ar = zip::ZipArchive::new(file).map_err(|e| format!("无法读取归档：{e}"))?;
    for i in 0..ar.len() {
        let mut entry = ar.by_index(i).map_err(|e| format!("归档条目损坏：{e}"))?;
        let Some(name) = entry.enclosed_name() else {
            return Err("归档条目逃逸目标目录".to_string());
        };
        let out = confine(dest, &name)?;
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("无法创建目录：{e}"))?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("无法创建目录：{e}"))?;
            }
            let mut out_file = fs::File::create(&out).map_err(|e| format!("无法写入条目：{e}"))?;
            std::io::copy(&mut entry, &mut out_file).map_err(|e| format!("无法解压条目：{e}"))?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode & 0o7777));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ct_extract_{}_{}_{tag}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn strip_components() {
        assert_eq!(
            strip_first_components(Path::new("python/bin/x"), 1),
            Some(PathBuf::from("bin/x"))
        );
        assert_eq!(strip_first_components(Path::new("python"), 1), None);
        assert_eq!(strip_first_components(Path::new("/abs/x"), 1), None);
        assert_eq!(strip_first_components(Path::new("a"), 1), None);
    }

    #[test]
    fn confine_rejects_escape() {
        let dest = Path::new("/dist");
        assert!(confine(dest, Path::new("a/b")).is_ok());
        assert!(confine(dest, Path::new("../evil")).is_err());
        assert!(confine(dest, Path::new("/abs")).is_err());
    }

    #[test]
    fn unsupported_format() {
        let err = extract(Path::new("x.7z"), Path::new("/tmp")).unwrap_err();
        assert!(err.contains("不支持"));
    }

    fn make_tar_gz(path: &Path) {
        let file = std::fs::File::create(path).unwrap();
        let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);
        let data = b"hi";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "topdir/bin/tool", data.as_slice())
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn tar_gz_strips_top_dir() {
        let dir = unique_dir("tar");
        let archive = dir.join("a.tar.gz");
        make_tar_gz(&archive);
        let dest = dir.join("out");
        extract(&archive, &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("bin/tool")).unwrap(), b"hi");
        assert!(!dest.join("topdir").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn confine_rejects_nested_escape() {
        // 形如剥掉顶目录后残留的 `a/../../evil` 同样逃逸。
        let dest = Path::new("/dist");
        assert!(confine(dest, Path::new("a/../../evil")).is_err());
        assert!(confine(dest, Path::new("a/b")).is_ok());
    }

    #[test]
    fn zip_roundtrip() {
        let dir = unique_dir("zip");
        let archive = dir.join("a.zip");
        {
            let file = std::fs::File::create(&archive).unwrap();
            let mut w = zip::ZipWriter::new(file);
            w.start_file("uv", zip::write::SimpleFileOptions::default())
                .unwrap();
            use std::io::Write;
            w.write_all(b"binary").unwrap();
            w.finish().unwrap();
        }
        let dest = dir.join("out");
        extract(&archive, &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("uv")).unwrap(), b"binary");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
