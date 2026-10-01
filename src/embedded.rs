//! 内建默认资源：编译期编入可执行文件的脚本与默认配置。
//!
//! 清单由 build.rs 扫描 `templates/` 生成；查找键为相对 `templates/`
//! 的正斜杠路径（如 `scripts/check_update.bat`），与分发目录布局对齐。

include!(concat!(env!("OUT_DIR"), "/embedded_manifest.rs"));

/// 内建默认配置：`launcher.template.toml` 的编译期快照。
/// 外部 `launcher.toml` 缺失时回退到它（仍需按真实分发修改后才能跑通）。
pub const DEFAULT_LAUNCHER_TOML: &str = include_str!("../launcher.template.toml");

/// 按键取内建文件内容；外部同名文件优先，取不到才走这里。
pub fn get(name: &str) -> Option<&'static str> {
    let key = name.replace('\\', "/");
    let key = key.strip_prefix("./").unwrap_or(&key);
    EMBEDDED_FILES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, content)| *content)
}

/// 内建键全集，供配置层判定"外部缺失时是否有内建回退"。
pub fn contains(name: &str) -> bool {
    get(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn templates_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates")
    }

    #[test]
    fn manifest_matches_disk_templates() {
        assert!(
            !EMBEDDED_FILES.is_empty(),
            "templates/ 下应至少有一个内建文件"
        );
        for (key, content) in EMBEDDED_FILES {
            let disk = std::fs::read_to_string(templates_dir().join(key))
                .unwrap_or_else(|_| panic!("内建键 {key} 在磁盘上不存在"));
            assert_eq!(content, &disk, "内建内容与磁盘模板不一致：{key}");
        }
    }

    #[test]
    fn default_config_parses() {
        let val: toml::Value =
            toml::from_str(DEFAULT_LAUNCHER_TOML).expect("内建默认配置应为合法 TOML");
        assert!(val.get("hooks").is_some(), "内建默认配置应包含 [hooks]");
    }

    #[test]
    fn lookup_normalizes_separators() {
        assert!(contains("scripts/check_update.bat"));
        assert!(contains("scripts\\check_update.bat"));
        assert!(contains("./scripts/check_update.bat"));
        assert!(!contains("scripts/does-not-exist.bat"));
    }
}
