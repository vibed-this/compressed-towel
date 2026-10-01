//! uv 供给与单包安装：uv 强制，无 pip 回退，失败即整轮失败。

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::runner::UiEvent;

use super::source::{self, ArchiveRef};
use super::{supply_archive, verify};

fn uv_exe_name() -> &'static str {
    if cfg!(windows) { "uv.exe" } else { "uv" }
}

/// uv 落盘目录（confinement 内，与运行时分离）。
fn tools_dir(config: &crate::config::Config) -> PathBuf {
    config.exe_dir.join("tools")
}

fn uv_fingerprint() -> String {
    let triple = source::platform_triple().unwrap_or("unknown");
    format!("{}/{triple}", source::UV_VERSION)
}

/// 确保 uv 可用，返回其绝对路径；已供给且指纹命中则直接复用。
pub fn ensure_uv(
    config: &crate::config::Config,
    _py: &super::PythonConfig,
    tx: &Sender<UiEvent>,
) -> Result<PathBuf, String> {
    let exe = tools_dir(config).join(uv_exe_name());
    let fingerprint = uv_fingerprint();
    if verify::marker_matches(&config.exe_dir, "uv", &fingerprint) && exe.is_file() {
        return Ok(exe);
    }
    let archive: ArchiveRef = source::uv_archive()?;
    supply_archive(
        &archive,
        &tools_dir(config),
        &config.exe_dir,
        "uv",
        &fingerprint,
        tx,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&exe)
            .map_err(|_| "uv 解压后仍缺失可执行文件".to_string())?
            .permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&exe, perm).map_err(|e| format!("无法设置 uv 执行位：{e}"))?;
    }
    if !exe.is_file() {
        return Err("uv 解压后仍缺失可执行文件".to_string());
    }
    Ok(exe)
}

/// 安装业务单包：`uv pip install --python <runtime> --only-binary :all: …`。
pub fn install_package(
    config: &crate::config::Config,
    py: &super::PythonConfig,
    uv_exe: &Path,
    tx: &Sender<UiEvent>,
) -> Result<(), String> {
    let runtime_abs = config.exe_dir.join(&py.runtime_path);
    let _ = tx.send(UiEvent::OutputLine {
        line: format!("[python] installing {} …", py.package),
    });
    let mut argv: Vec<String> = vec![
        uv_exe.to_string_lossy().into_owned(),
        "pip".to_string(),
        "install".to_string(),
        "--python".to_string(),
        runtime_abs.to_string_lossy().into_owned(),
        "--only-binary".to_string(),
        ":all:".to_string(),
    ];
    if !py.is_direct_url() {
        argv.push("--index-url".to_string());
        argv.push(py.index_url.clone());
        if let Some(host) = py.trusted_host.as_deref() {
            argv.push("--trusted-host".to_string());
            argv.push(host.to_string());
        }
    }
    argv.push(py.package.clone());

    match crate::runner::run_captured(config, &argv, tx) {
        Some(0) => Ok(()),
        Some(code) => Err(format!(
            "uv 安装失败，退出码 {code}（index：{}）",
            py.redacted_index_url()
        )),
        None => Err("uv 启动失败".to_string()),
    }
}
