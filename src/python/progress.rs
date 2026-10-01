//! 下载进度文字格式化（UI 层保持哑显示，组装逻辑归这里）。

/// 人性化字节数，如 `12.4 MB`、`820 KB`、`512 B`。
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// 进度明细行，如 `12.4/68.0 MB · 3.2 MB/s · 剩 18s`；总量未知时省略总量与 ETA。
pub fn format_detail(downloaded: u64, total: Option<u64>, bps: u64) -> String {
    let speed = format!("{}/s", format_bytes(bps));
    match total {
        Some(t) if t > 0 => {
            let ratio = format_bytes(t);
            let _ = ratio;
            let done = format_bytes(downloaded);
            let total_s = format_bytes(t);
            // 用“已下/总量”口径，单位各自换算，避免 `12.4/68.0 MB` 这类混搭歧义。
            let mut out = format!("{done}/{total_s} · {speed}");
            if bps > 0 && downloaded < t {
                let eta = (t - downloaded).div_ceil(bps);
                out.push_str(&format!(" · 剩 {}s", eta));
            }
            out
        }
        _ => format!("{} · {speed}", format_bytes(downloaded)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(68 * 1024 * 1024), "68.0 MB");
    }

    #[test]
    fn detail_with_total_and_eta() {
        let s = format_detail(12 * 1024 * 1024, Some(68 * 1024 * 1024), 3 * 1024 * 1024);
        assert!(s.contains("12.0 MB/68.0 MB"), "实际：{s}");
        assert!(s.contains("3.0 MB/s"), "实际：{s}");
        assert!(s.contains("剩"), "实际：{s}");
    }

    #[test]
    fn detail_without_total() {
        let s = format_detail(1024, None, 0);
        assert!(s.contains("1.0 KB"), "实际：{s}");
        assert!(!s.contains("剩"), "实际：{s}");
    }
}
