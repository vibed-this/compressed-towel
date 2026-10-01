//! 校验与原子落盘：sha256 比对、`.part` 原子改名、`.provision-ok` 幂等标记。

use std::path::{Path, PathBuf};

/// sha256 十六进制比对（大小写不敏感，容忍首尾空白）。
pub fn check_sha256(actual_hex: &str, expected_hex: &str) -> Result<(), String> {
    if actual_hex.trim().eq_ignore_ascii_case(expected_hex.trim()) {
        Ok(())
    } else {
        Err("sha256 不一致，文件可能损坏或被篡改".to_string())
    }
}

/// 幂等标记路径：`<dir>/.provision-ok-<tag>`，内容为版本指纹。
pub fn marker_path(dir: &Path, tag: &str) -> PathBuf {
    dir.join(format!(".provision-ok-{tag}"))
}

pub fn marker_matches(dir: &Path, tag: &str, fingerprint: &str) -> bool {
    std::fs::read_to_string(marker_path(dir, tag))
        .map(|s| s.trim() == fingerprint)
        .unwrap_or(false)
}

pub fn write_marker(dir: &Path, tag: &str, fingerprint: &str) -> Result<(), String> {
    if let Err(e) = std::fs::create_dir_all(dir) {
        return Err(format!("无法创建目录 {}: {e}", dir.display()));
    }
    std::fs::write(marker_path(dir, tag), fingerprint).map_err(|e| format!("无法写入供给标记: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_case_insensitive() {
        assert!(check_sha256("ABCDEF", "abcdef").is_ok());
        assert!(check_sha256("abc", "abd").is_err());
    }

    #[test]
    fn marker_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "ct_verify_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        assert!(!marker_matches(&dir, "python", "v1"));
        write_marker(&dir, "python", "v1").unwrap();
        assert!(marker_matches(&dir, "python", "v1"));
        assert!(!marker_matches(&dir, "python", "v2"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
