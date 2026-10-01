//! Python 供给：便携运行时 + uv + 单包远端 wheel 安装。
//!
//! 缺配即通用：`[python]` 未配置或 `enable = false` 时本模块不参与，
//! 启动器行为与通用模式完全一致。

mod config;
mod extract;
mod fetch;
mod progress;
mod source;
mod uv;
mod verify;

pub use config::{PythonConfig, PythonSource};
pub use progress::format_detail;

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::runner::UiEvent;

/// 供给总入口：运行时 → uv → 业务单包，任一步失败即整轮失败。
pub fn ensure_python_stack(
    config: &crate::config::Config,
    tx: &Sender<UiEvent>,
) -> Result<(), String> {
    let py = config
        .python
        .as_ref()
        .ok_or_else(|| "内部错误：python 未配置".to_string())?;
    ensure_runtime(config, py, tx)?;
    let uv_exe = uv::ensure_uv(config, py, tx)?;
    uv::install_package(config, py, &uv_exe, tx)?;
    Ok(())
}

/// 通用归档供给：下载 → sha256 → 解压 → 幂等标记；结束时上报 `ProvisionDone`。
fn supply_archive(
    archive: &source::ArchiveRef,
    dest_dir: &Path,
    marker_dir: &Path,
    marker_tag: &str,
    fingerprint: &str,
    tx: &Sender<UiEvent>,
) -> Result<(), String> {
    let part = download_part_path(&archive.file_name);
    let digest =
        fetch::download_to_part(&archive.url, &part, &archive.file_name, tx).inspect_err(|_| {
            let _ = std::fs::remove_file(&part);
        })?;
    if let Err(e) = verify::check_sha256(&digest.sha256_hex, &archive.sha256) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    let result = extract::extract(&part, dest_dir);
    let _ = std::fs::remove_file(&part);
    result?;
    verify::write_marker(marker_dir, marker_tag, fingerprint)?;
    let _ = tx.send(UiEvent::ProvisionDone {
        file: archive.file_name.clone(),
    });
    Ok(())
}

fn download_part_path(file_name: &str) -> PathBuf {
    let safe: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    std::env::temp_dir().join(format!("ct-dl-{}-{safe}.part", std::process::id()))
}

fn ensure_runtime(
    config: &crate::config::Config,
    py: &PythonConfig,
    tx: &Sender<UiEvent>,
) -> Result<(), String> {
    if py.source == PythonSource::System {
        return check_system_runtime(config, py);
    }
    let archive = source::python_archive(
        py.source,
        &py.version,
        py.custom_url.as_deref(),
        py.custom_sha256.as_deref(),
    )?
    .ok_or_else(|| "内部错误：该源无需下载".to_string())?;
    let fingerprint = runtime_fingerprint(py);
    let runtime_abs = config.exe_dir.join(&py.runtime_path);
    if verify::marker_matches(&config.exe_dir, "python", &fingerprint) && runtime_abs.is_file() {
        return Ok(());
    }
    supply_archive(
        &archive,
        &config.exe_dir,
        &config.exe_dir,
        "python",
        &fingerprint,
        tx,
    )?;
    if !runtime_abs.is_file() {
        return Err(format!(
            "运行时解压后仍缺失：{}（检查 runtime_path 是否与归档布局一致）",
            py.runtime_path
        ));
    }
    Ok(())
}

fn runtime_fingerprint(py: &PythonConfig) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    match py.source {
        PythonSource::Custom => {
            let mut h = DefaultHasher::new();
            py.custom_url.as_deref().unwrap_or_default().hash(&mut h);
            py.custom_sha256.as_deref().unwrap_or_default().hash(&mut h);
            format!("custom/{:x}", h.finish())
        }
        _ => {
            let triple = source::platform_triple().unwrap_or("unknown");
            format!("{}/{}/{triple}", py.source.as_str(), py.version)
        }
    }
}

/// `system` 模式：只校验本机运行时存在且大版本不低于配置。
fn check_system_runtime(config: &crate::config::Config, py: &PythonConfig) -> Result<(), String> {
    let program = config
        .resolve_program(&py.runtime_path)
        .map_err(|e| format!("system 运行时解析失败：{e}"))?;
    let out = std::process::Command::new(&program)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("无法执行 system 运行时：{e}"))?;
    if !out.status.success() {
        return Err("system 运行时 `--version` 失败".to_string());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let found = parse_python_version(&text).ok_or_else(|| format!("无法识别运行时版本：{text}"))?;
    let want = parse_python_version(&py.version).unwrap_or((3, 0));
    if found < want {
        return Err(format!(
            "system 运行时版本过低：实际 {found:?}，要求至少 {want:?}"
        ));
    }
    Ok(())
}

/// 从 `Python 3.12.1` 这类输出解析出 `(major, minor)`。
fn parse_python_version(text: &str) -> Option<(u32, u32)> {
    let mut nums = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u32>().ok());
    Some((nums.next()?, nums.next()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsed() {
        assert_eq!(parse_python_version("Python 3.12.14"), Some((3, 12)));
        assert_eq!(parse_python_version("Python 3.12.14\n"), Some((3, 12)));
        assert!(parse_python_version("no version here").is_none());
    }
}
