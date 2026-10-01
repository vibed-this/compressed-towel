//! `[python]` 配置节解析。
//!
//! 单包远端模式：只装一个业务包，来源为官方 PyPI / 自定义 pip server / 直接 URL。

use std::path::Path;

use crate::config::ConfigError;

/// 默认 Python 版本（显式 pin，保证分发可复现；内建归档见 source.rs）。
pub const DEFAULT_VERSION: &str = "3.12.14";
/// 缺配即官方 PyPI。
pub const DEFAULT_INDEX_URL: &str = "https://pypi.org/simple";

/// Python 供给源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonSource {
    /// astral-sh/python-build-standalone：默认，全平台。
    Pbs,
    /// WinPython：仅 Windows 的 legacy 选项。
    Winpython,
    /// 本机 Python：只做版本下限检查，不下载。
    System,
    /// 自定义归档 URL：内网/镜像场景。
    Custom,
}

impl PythonSource {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "python-build-standalone" => Some(Self::Pbs),
            "winpython" => Some(Self::Winpython),
            "system" => Some(Self::System),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pbs => "python-build-standalone",
            Self::Winpython => "winpython",
            Self::System => "system",
            Self::Custom => "custom",
        }
    }
}

/// 各平台默认运行时相对路径（confinement 内）。
pub fn default_runtime_path() -> &'static str {
    if cfg!(windows) {
        "python/python.exe"
    } else {
        "python/bin/python3"
    }
}

#[derive(Debug, Clone)]
pub struct PythonConfig {
    pub version: String,
    pub source: PythonSource,
    pub custom_url: Option<String>,
    pub custom_sha256: Option<String>,
    pub runtime_path: String,
    /// 唯一业务包 spec，如 `myapp==1.2.3` 或 `myapp @ https://.../*.whl`。
    pub package: String,
    pub index_url: String,
    /// 仅 http 内网源需要透传给 uv/pip。
    pub trusted_host: Option<String>,
}

fn get_bool(table: &toml::Table, key: &str, file: &Path) -> Result<Option<bool>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Boolean(b)) => Ok(Some(*b)),
        Some(_) => Err(ConfigError::Invalid {
            file: file.to_path_buf(),
            message: format!("[python] '{key}' must be a boolean"),
        }),
    }
}

fn get_string(table: &toml::Table, key: &str, file: &Path) -> Result<Option<String>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ConfigError::Invalid {
            file: file.to_path_buf(),
            message: format!("[python] '{key}' must be a string"),
        }),
    }
}

impl PythonConfig {
    /// 解析 `[python]` 节：节缺失或 `enable = false` 时返回 `None`（通用模式）。
    pub fn parse(
        root: &toml::Table,
        file: &Path,
        resolved: &std::collections::HashMap<String, String>,
    ) -> Result<Option<Self>, ConfigError> {
        let table = match root.get("python") {
            None => return Ok(None),
            Some(toml::Value::Table(t)) => t,
            Some(_) => {
                return Err(ConfigError::Invalid {
                    file: file.to_path_buf(),
                    message: "[python] must be a table".to_string(),
                });
            }
        };
        if !get_bool(table, "enable", file)?.unwrap_or(false) {
            return Ok(None);
        }

        let version =
            get_string(table, "version", file)?.unwrap_or_else(|| DEFAULT_VERSION.to_string());
        if version.trim().is_empty() {
            return Err(ConfigError::Invalid {
                file: file.to_path_buf(),
                message: "[python] 'version' must not be empty".to_string(),
            });
        }
        let source_raw =
            get_string(table, "source", file)?.unwrap_or_else(|| "python-build-standalone".into());
        let source = PythonSource::parse(&source_raw).ok_or_else(|| ConfigError::Invalid {
            file: file.to_path_buf(),
            message: format!(
                "[python] unknown source '{source_raw}', expected one of: python-build-standalone | winpython | system | custom"
            ),
        })?;

        let expand = |s: &str| crate::config::expand_with_resolved(s, resolved);
        let custom_url = get_string(table, "url", file)?.map(|s| expand(&s));
        let custom_sha256 = get_string(table, "sha256", file)?.map(|s| expand(&s));
        if source == PythonSource::Custom {
            match (&custom_url, &custom_sha256) {
                (Some(u), Some(h)) if !u.trim().is_empty() && !h.trim().is_empty() => {}
                _ => {
                    return Err(ConfigError::Invalid {
                        file: file.to_path_buf(),
                        message: "[python] source 'custom' requires non-empty 'url' and 'sha256'"
                            .to_string(),
                    });
                }
            }
        }

        let runtime_path = get_string(table, "runtime_path", file)?
            .map(|s| expand(&s))
            .unwrap_or_else(|| default_runtime_path().to_string());
        if runtime_path.trim().is_empty() {
            return Err(ConfigError::Invalid {
                file: file.to_path_buf(),
                message: "[python] 'runtime_path' must not be empty".to_string(),
            });
        }

        let package = get_string(table, "package", file)?
            .map(|s| expand(&s))
            .unwrap_or_default();
        let package = package.trim().to_string();
        if package.is_empty() || package.contains('\n') {
            return Err(ConfigError::Invalid {
                file: file.to_path_buf(),
                message: "[python] 'package' must be a single non-empty package spec".to_string(),
            });
        }

        let index_url = get_string(table, "index_url", file)?
            .map(|s| expand(&s))
            .unwrap_or_else(|| DEFAULT_INDEX_URL.to_string());
        let trusted_host = get_string(table, "trusted_host", file)?.map(|s| expand(&s));

        Ok(Some(Self {
            version,
            source,
            custom_url,
            custom_sha256,
            runtime_path,
            package,
            index_url,
            trusted_host,
        }))
    }

    /// 是否为 PEP 508 直接 URL 引用（此时忽略 `index_url`）。
    pub fn is_direct_url(&self) -> bool {
        match self.package.split_once('@') {
            Some((_, uri)) => uri.trim_start().starts_with("http"),
            None => false,
        }
    }

    /// 脱敏后的 index（隐藏 `https://user:pass@host` 中的 userinfo）。
    pub fn redacted_index_url(&self) -> String {
        redact_userinfo(&self.index_url)
    }
}

fn redact_userinfo(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let rest = &url[scheme_end + 3..];
    let Some(at) = rest.find('@') else {
        return url.to_string();
    };
    format!("{}://***@{}", &url[..scheme_end], &rest[at + 1..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_toml(text: &str) -> Result<Option<PythonConfig>, ConfigError> {
        let val: toml::Value = toml::from_str(text).unwrap();
        let root = val.as_table().unwrap();
        PythonConfig::parse(root, Path::new("launcher.toml"), &Default::default())
    }

    #[test]
    fn missing_section_means_generic() {
        let cfg = parse_toml("[hooks]\nstart = [\"run\"]\n").unwrap();
        assert!(cfg.is_none());
    }

    #[test]
    fn disabled_means_generic() {
        let cfg = parse_toml("[python]\nenable = false\npackage = \"myapp==1.0\"\n").unwrap();
        assert!(cfg.is_none());
    }

    #[test]
    fn minimal_enable_applies_defaults() {
        let cfg = parse_toml("[python]\nenable = true\npackage = \"myapp==1.2.3\"\n").unwrap();
        let cfg = cfg.expect("应启用");
        assert_eq!(cfg.version, DEFAULT_VERSION);
        assert_eq!(cfg.source, PythonSource::Pbs);
        assert_eq!(cfg.index_url, DEFAULT_INDEX_URL);
        assert_eq!(cfg.runtime_path, default_runtime_path());
        assert!(!cfg.is_direct_url());
    }

    #[test]
    fn custom_source_requires_url_and_sha() {
        let err = parse_toml("[python]\nenable = true\nsource = \"custom\"\npackage = \"a==1\"\n")
            .unwrap_err();
        assert!(err.to_string().contains("'url' and 'sha256'"));
    }

    #[test]
    fn unknown_source_rejected() {
        let err = parse_toml("[python]\nenable = true\nsource = \"conda\"\npackage = \"a==1\"\n")
            .unwrap_err();
        assert!(err.to_string().contains("unknown source"));
    }

    #[test]
    fn empty_package_rejected() {
        let err = parse_toml("[python]\nenable = true\npackage = \"  \"\n").unwrap_err();
        assert!(err.to_string().contains("'package'"));
    }

    #[test]
    fn direct_url_detected() {
        let cfg = parse_toml(
            "[python]\nenable = true\npackage = \"myapp @ https://files.example/myapp-1.0-py3-none-any.whl\"\n",
        )
        .unwrap()
        .unwrap();
        assert!(cfg.is_direct_url());
    }

    #[test]
    fn index_url_userinfo_redacted() {
        let cfg = parse_toml(
            "[python]\nenable = true\npackage = \"a==1\"\nindex_url = \"https://token123@mirror.example/simple\"\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            cfg.redacted_index_url(),
            "https://***@mirror.example/simple"
        );
        assert!(cfg.redacted_index_url().contains("mirror.example"));
        assert!(!cfg.redacted_index_url().contains("token123"));
    }
}
