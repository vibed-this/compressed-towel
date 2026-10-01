//! 供给源矩阵：官方归档 URL 与 sha256 内建表（TLS + sha 双保险）。
//!
//! 数据来源（已逐项联网验证，勿手改）：python-build-standalone release
//! `20260929` 的资产名与 `SHA256SUMS`，astral-sh/uv `0.12.21` 的资产名与
//! 各 `.sha256` 文件。新增版本时在下表加一行，url 与 sha 一起 pin。

use super::config::PythonSource;

/// 当前平台三元组（与内建表键对齐）；不支持的平台返回 `None`。
pub fn platform_triple() -> Option<&'static str> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        Some("x86_64-pc-windows-msvc")
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        Some("aarch64-apple-darwin")
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        Some("x86_64-apple-darwin")
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Some("x86_64-unknown-linux-gnu")
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
        Some("aarch64-unknown-linux-gnu")
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveRef {
    /// 显示用文件名（同时用于本地 `.part` 后缀推断格式）。
    pub file_name: String,
    pub url: String,
    pub sha256: String,
}

/// （python 版本, 平台三元组, 资产文件名, sha256）。
const PBS_TABLE: &[(&str, &str, &str, &str)] = &[
    (
        "3.12.14",
        "x86_64-pc-windows-msvc",
        "cpython-3.12.14+20260929-x86_64-pc-windows-msvc-install_only.tar.gz",
        "28728baf30b65e263f0b25c5a85be8226e7ab0d212fbadd6a8f0f796139fa804",
    ),
    (
        "3.12.14",
        "x86_64-apple-darwin",
        "cpython-3.12.14+20260929-x86_64-apple-darwin-install_only.tar.gz",
        "f51ec8a7fa0ede129a5e2e942a2a453ba4830adaca2289da4449390e42443292",
    ),
    (
        "3.12.14",
        "aarch64-apple-darwin",
        "cpython-3.12.14+20260929-aarch64-apple-darwin-install_only.tar.gz",
        "de6b8f94fa765639b423ea353ab340669704c7186f96ee3cab389dcfde770c3c",
    ),
    (
        "3.12.14",
        "x86_64-unknown-linux-gnu",
        "cpython-3.12.14+20260929-x86_64-unknown-linux-gnu-install_only.tar.gz",
        "06c90b93f419b63371c18f20fed0558a1a901f6518c3c24f755077e048447e7f",
    ),
    (
        "3.12.14",
        "aarch64-unknown-linux-gnu",
        "cpython-3.12.14+20260929-aarch64-unknown-linux-gnu-install_only.tar.gz",
        "9c797cf657f6dced51d3e74eeabd7c1ca742d5bf10f66080a290e424ced8edaf",
    ),
];

const PBS_RELEASE: &str = "20260929";

fn pbs_url(asset: &str) -> String {
    format!(
        "https://github.com/astral-sh/python-build-standalone/releases/download/{PBS_RELEASE}/{asset}"
    )
}

/// 内建表支持的 python 版本（报错时提示用）。
pub fn supported_python_versions() -> Vec<&'static str> {
    let mut out = Vec::new();
    for (v, _, _, _) in PBS_TABLE {
        if !out.contains(v) {
            out.push(*v);
        }
    }
    out
}

/// 解析运行时归档：`System` 由调用方另行处理（返回 `None` 表示无需下载）。
pub fn python_archive(
    source: PythonSource,
    version: &str,
    custom_url: Option<&str>,
    custom_sha256: Option<&str>,
) -> Result<Option<ArchiveRef>, String> {
    match source {
        PythonSource::System => Ok(None),
        PythonSource::Custom => {
            let (url, sha256) = match (custom_url, custom_sha256) {
                (Some(u), Some(s)) => (u, s),
                _ => return Err("[python] source 'custom' 需要 'url' 与 'sha256'".to_string()),
            };
            Ok(Some(ArchiveRef {
                file_name: url_basename(url)?,
                url: url.to_string(),
                sha256: sha256.to_string(),
            }))
        }
        PythonSource::Winpython => Err(
            "source 'winpython' 暂未内建归档表，请改用 source = \"custom\" 并提供 url/sha256"
                .to_string(),
        ),
        PythonSource::Pbs => {
            let triple = platform_triple().ok_or_else(|| {
                "当前平台暂无内建便携 Python，请用 source = \"custom\"".to_string()
            })?;
            for (v, t, asset, sha) in PBS_TABLE {
                if *v == version && *t == triple {
                    return Ok(Some(ArchiveRef {
                        file_name: asset.to_string(),
                        url: pbs_url(asset),
                        sha256: sha.to_string(),
                    }));
                }
            }
            Err(format!(
                "python-build-standalone 内建表不支持版本 '{version}'，可用版本：{}；或用 source = \"custom\"",
                supported_python_versions().join(", ")
            ))
        }
    }
}

fn url_basename(url: &str) -> Result<String, String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let base = path.rsplit('/').next().unwrap_or(path);
    if base.is_empty() || !base.contains('.') {
        return Err(format!("无法从 url 推断文件名：{url}"));
    }
    Ok(base.to_string())
}

/// uv 版本（代码 pin，不暴露配置）。
pub const UV_VERSION: &str = "0.12.21";
const UV_BASE: &str = "https://github.com/astral-sh/uv/releases/download/0.12.21";

/// （平台三元组, 资产文件名, sha256）。
const UV_TABLE: &[(&str, &str, &str)] = &[
    (
        "x86_64-pc-windows-msvc",
        "uv-x86_64-pc-windows-msvc.zip",
        "5d223efa0bf00208c3853246af09420419dfbd352536aa6bb8163d6170e23890",
    ),
    (
        "x86_64-unknown-linux-gnu",
        "uv-x86_64-unknown-linux-gnu.tar.gz",
        "23f02075b652bb1df64178cfae41b5caf160822e720e2663568f3f5d63bc52c0",
    ),
    (
        "aarch64-apple-darwin",
        "uv-aarch64-apple-darwin.tar.gz",
        "b88bda573e566ef9bced66b155fe0408626fbbc053aee1c30ba686f0728c9447",
    ),
    (
        "x86_64-apple-darwin",
        "uv-x86_64-apple-darwin.tar.gz",
        "2b336763b396ec6afa20c5a8b083538ca7402445b868311979d740a4344c17d8",
    ),
    (
        "aarch64-unknown-linux-gnu",
        "uv-aarch64-unknown-linux-gnu.tar.gz",
        "030b69227b40af8c1981b7301793dc66e71ed3c796ea8688209dd268bd91ec51",
    ),
];

pub fn uv_archive() -> Result<ArchiveRef, String> {
    let triple = platform_triple()
        .ok_or_else(|| "当前平台暂无内建 uv，请用 source = \"custom\" 自备 uv".to_string())?;
    for (t, asset, sha) in UV_TABLE {
        if *t == triple {
            return Ok(ArchiveRef {
                file_name: asset.to_string(),
                url: format!("{UV_BASE}/{asset}"),
                sha256: sha.to_string(),
            });
        }
    }
    Err("当前平台暂无内建 uv，请用 source = \"custom\" 自备 uv".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_self_consistent() {
        for (v, t, asset, sha) in PBS_TABLE {
            assert!(asset.contains(v), "资产名应含版本：{asset}");
            assert!(asset.contains(t), "资产名应含平台：{asset}");
            assert_eq!(sha.len(), 64, "sha256 应为 64 位 hex：{asset}");
            assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        }
        for (t, asset, sha) in UV_TABLE {
            assert!(asset.contains(t), "资产名应含平台：{asset}");
            assert_eq!(sha.len(), 64);
        }
        assert!(!supported_python_versions().is_empty());
    }

    #[test]
    fn system_needs_no_download() {
        let r = python_archive(PythonSource::System, "3.12.14", None, None).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn winpython_guides_to_custom() {
        let err = python_archive(PythonSource::Winpython, "3.12.14", None, None).unwrap_err();
        assert!(err.contains("custom"));
    }

    #[test]
    fn unknown_version_lists_supported() {
        let err = python_archive(PythonSource::Pbs, "3.9.0", None, None).unwrap_err();
        assert!(err.contains("3.12.14"), "实际：{err}");
    }
}
