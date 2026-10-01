//! ureq 流式下载：边下边算 sha256，节流上报进度事件。
//!
//! 代理复用父进程环境（`HTTPS_PROXY`/`HTTP_PROXY` 大小写均可），不新增配置项。

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::runner::UiEvent;

const CHUNK_BYTES: usize = 64 * 1024;
/// 进度上报节流间隔：避免高频跨线程事件淹没界面循环。
const REPORT_INTERVAL: Duration = Duration::from_millis(100);

pub struct FetchResult {
    pub sha256_hex: String,
}

fn proxy_from_env() -> Option<String> {
    for key in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

fn agent() -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(600)));
    if let Some(proxy_url) = proxy_from_env()
        && let Ok(proxy) = ureq::Proxy::new(&proxy_url)
    {
        builder = builder.proxy(Some(proxy));
    }
    ureq::Agent::new_with_config(builder.build())
}

/// 滑动窗口速率：窗口满 1s 用窗口速率，否则用全程平均（不足 0.5s 视为 0）。
fn bps_for_window(window_bytes: u64, window_secs: f64, downloaded: u64, total_secs: f64) -> u64 {
    if window_secs >= 1.0 {
        (window_bytes as f64 / window_secs) as u64
    } else if total_secs >= 0.5 {
        (downloaded as f64 / total_secs) as u64
    } else {
        0
    }
}

/// 下载 `url` 到 `dest_part`（调用方给 `.part` 后缀路径），以 `display_name` 上报进度。
pub fn download_to_part(
    url: &str,
    dest_part: &Path,
    display_name: &str,
    tx: &Sender<UiEvent>,
) -> Result<FetchResult, String> {
    let mut response = agent()
        .get(url)
        .call()
        .map_err(|e| format!("下载失败 {url}: {e}"))?;
    let total = response.body().content_length();
    let _ = tx.send(UiEvent::ProvisionStarted {
        file: display_name.to_string(),
        total,
    });

    if let Some(parent) = dest_part.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建目录 {}: {e}", parent.display()))?;
    }
    let file = std::fs::File::create(dest_part)
        .map_err(|e| format!("无法写入 {}: {e}", dest_part.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    let mut reader = response.body_mut().as_reader();

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut downloaded: u64 = 0;
    let start = Instant::now();
    let mut last_report = start;
    let mut window_start = start;
    let mut window_bytes: u64 = 0;

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("读取 {display_name} 失败: {e}"))?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .map_err(|e| format!("写入 {display_name} 失败: {e}"))?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;
        window_bytes += n as u64;

        let now = Instant::now();
        if now.duration_since(last_report) >= REPORT_INTERVAL {
            let bps = bps_for_window(
                window_bytes,
                window_start.elapsed().as_secs_f64(),
                downloaded,
                start.elapsed().as_secs_f64(),
            );
            window_start = now;
            window_bytes = 0;
            last_report = now;
            let _ = tx.send(UiEvent::ProvisionProgress {
                file: display_name.to_string(),
                downloaded,
                total,
                bps,
            });
        }
    }
    writer
        .flush()
        .map_err(|e| format!("写入 {display_name} 失败: {e}"))?;

    let bps = bps_for_window(
        window_bytes,
        window_start.elapsed().as_secs_f64(),
        downloaded,
        start.elapsed().as_secs_f64(),
    );
    let _ = tx.send(UiEvent::ProvisionProgress {
        file: display_name.to_string(),
        downloaded,
        total: Some(total.unwrap_or(downloaded)),
        bps,
    });

    Ok(FetchResult {
        sha256_hex: format!("{:x}", hasher.finalize()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bps_prefers_window_when_full() {
        // 2s 窗口 6MB → 3MB/s，不看全程平均。
        assert_eq!(bps_for_window(6_000_000, 2.0, 60_000_000, 20.0), 3_000_000);
    }

    #[test]
    fn bps_falls_back_to_average() {
        // 窗口不足 1s，全程 10s 下了 20MB → 2MB/s。
        assert_eq!(bps_for_window(1_000_000, 0.2, 20_000_000, 10.0), 2_000_000);
    }

    #[test]
    fn bps_zero_at_start() {
        assert_eq!(bps_for_window(100, 0.01, 100, 0.01), 0);
    }

    /// 联网冒烟：下载百字节级 `.sha256`，验证 ureq 全链路与内建表一致。
    /// 运行：`cargo test -- --ignored`（默认单测不碰网络）。
    #[test]
    #[ignore = "需联网"]
    fn smoke_download_uv_sha() {
        let (tx, rx) = std::sync::mpsc::channel();
        let archive = super::super::source::uv_archive().expect("当前平台应有内建 uv");
        let url = format!("{}.sha256", archive.url);
        let dest = std::env::temp_dir().join(format!("ct-smoke-{}.part", std::process::id()));
        let result = download_to_part(&url, &dest, "smoke.sha256", &tx).expect("下载应成功");
        let events: Vec<UiEvent> = rx.try_iter().collect();
        assert!(
            events.iter().any(|e| matches!(
                e,
                UiEvent::ProvisionStarted { file, .. } if file == "smoke.sha256"
            )),
            "应上报 Started"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, UiEvent::ProvisionProgress { .. })),
            "应上报 Progress"
        );
        let body = std::fs::read_to_string(&dest).unwrap();
        let pinned = body.split_whitespace().next().unwrap_or_default();
        assert_eq!(pinned, archive.sha256, "内建表 sha 应与官方一致");
        // 自洽：文件内容重算 sha 应等于返回摘要。
        let mut h = Sha256::new();
        h.update(body.as_bytes());
        assert_eq!(format!("{:x}", h.finalize()), result.sha256_hex);
        let _ = std::fs::remove_file(&dest);
    }
}
