mod config;
mod embedded;
mod python;
mod runner;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

slint::include_modules!();

const MAX_OUTPUT_LINES: usize = 500;

fn exe_dir() -> PathBuf {
    match std::env::current_exe() {
        Ok(exe) => resolve_exe_dir(&exe),
        Err(_) => PathBuf::from("."),
    }
}

/// 解析资源根目录：在 `Name.app/Contents/MacOS/<bin>` 内运行时指向
/// 同包的 `Contents/Resources`（打包脚本把配置与脚本放在那里）；
/// 其余情况指向可执行文件所在目录，裸包逻辑零影响。
fn resolve_exe_dir(exe: &Path) -> PathBuf {
    if let Some(macos) = exe.parent()
        && macos.file_name().is_some_and(|n| n == "MacOS")
        && let Some(contents) = macos.parent()
        && contents.file_name().is_some_and(|n| n == "Contents")
        && let Some(bundle) = contents.parent()
        && bundle.extension().is_some_and(|e| e == "app")
    {
        return contents.join("Resources");
    }
    exe.parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn hook_status(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.contains("update") || lower.contains("check") || name.contains("更新") {
        "正在检查更新…".to_string()
    } else if lower.contains("install") || lower.contains("download") || name.contains("安装") {
        "正在安装…".to_string()
    } else if lower.contains("clean") || name.contains("清理") {
        "正在清理…".to_string()
    } else if lower.contains("start") || lower.contains("launch") || name.contains("启动") {
        "正在启动…".to_string()
    } else {
        format!("正在执行：{name}…")
    }
}

fn push_output(ui: &App, line: &str) {
    let mut text = ui.get_output_text().to_string();
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(line);
    let count = text.lines().count();
    if count > MAX_OUTPUT_LINES {
        text = text
            .lines()
            .skip(count - MAX_OUTPUT_LINES)
            .collect::<Vec<_>>()
            .join("\n");
    }
    ui.set_output_text(text.into());
}

fn print_usage(program: &str) {
    println!("用法：{program} [--config <path>] [check]");
}

fn main() {
    let program = std::env::args()
        .next()
        .unwrap_or_else(|| "compressed-towel".into());
    let mut cli_config: Option<PathBuf> = None;
    let mut check_mode = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "check" {
            check_mode = true;
        } else if arg == "--config" {
            match args.next() {
                Some(value) => cli_config = Some(PathBuf::from(value)),
                None => {
                    eprintln!("缺少 --config 的值");
                    std::process::exit(2);
                }
            }
        } else if arg == "--help" || arg == "-h" {
            print_usage(&program);
            return;
        } else {
            eprintln!("未知参数：{arg}");
            print_usage(&program);
            std::process::exit(2);
        }
    }

    let dir = exe_dir();

    if check_mode {
        match config::Config::load(&dir, cli_config.as_deref()) {
            Ok(cfg) => {
                println!("应用：{}", cfg.app.name);
                match cfg.app.version.as_deref() {
                    Some(v) => println!("版本：{v}"),
                    None => println!("版本：未配置"),
                }
                println!("程序目录：{}", cfg.exe_dir.display());
                println!("配置文件：{}", cli_config_or_default(&cli_config));
                match cfg.splash.logo.as_deref() {
                    Some(logo) => println!("logo：{logo}"),
                    None => println!("logo：未配置"),
                }
                match cfg.resolved_env.get("RUNTIME") {
                    Some(runtime) => println!("RUNTIME：{runtime}"),
                    None => println!("RUNTIME：未配置"),
                }
                match cfg.python.as_ref() {
                    Some(py) => {
                        println!(
                            "python：source={} version={}",
                            py.source.as_str(),
                            py.version
                        );
                        println!("python package：{}", py.package);
                        println!("python index：{}", py.redacted_index_url());
                        println!("python runtime：{}", py.runtime_path);
                    }
                    None => println!("python：未启用"),
                }
                println!("日志等级：{}", cfg.launcher.log_level);
                if cfg.launcher.user_editable.is_empty() {
                    println!("用户可改项：无");
                } else {
                    println!("用户可改项：{}", cfg.launcher.user_editable.join(", "));
                }
                for (name, hook) in [
                    ("check_update", &cfg.hooks.check_update),
                    ("install", &cfg.hooks.install),
                    ("start", &cfg.hooks.start),
                    ("end", &cfg.hooks.end),
                ] {
                    match hook {
                        Some(argv) if !argv.is_empty() => {
                            println!("hook {name}：{}", argv.join(" "));
                        }
                        _ => println!("hook {name}：未配置"),
                    }
                }
                match verify_files(&cfg) {
                    Ok(()) => {
                        println!("校验通过");
                        cfg.cleanup_embedded();
                    }
                    Err(problems) => {
                        eprintln!("校验失败：\n{problems}");
                        cfg.cleanup_embedded();
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("配置错误：{e}");
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    let cfg = match config::Config::load(&dir, cli_config.as_deref()) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("配置错误：{e}");
            std::process::exit(2);
        }
    };

    let ui = match App::new() {
        Ok(ui) => ui,
        Err(e) => {
            eprintln!("界面初始化失败：{e}");
            std::process::exit(2);
        }
    };

    ui.set_app_name(cfg.app.name.clone().into());
    ui.set_status_text("正在准备…".into());
    ui.set_output_text("".into());
    ui.set_is_error(false);
    match cfg.splash.logo.as_deref() {
        Some(relative) => {
            let path = cfg.exe_dir.join(relative);
            match slint::Image::load_from_path(&path) {
                Ok(image) => {
                    ui.set_logo(image);
                    ui.set_has_logo(true);
                }
                Err(_) => ui.set_has_logo(false),
            }
        }
        None => ui.set_has_logo(false),
    }

    const EXIT_UNSET: i32 = i32::MIN;
    let exit_code = Arc::new(AtomicI32::new(EXIT_UNSET));
    {
        let exit_code = exit_code.clone();
        ui.on_close_clicked(move || {
            let _ = exit_code.compare_exchange(EXIT_UNSET, 1, Ordering::SeqCst, Ordering::SeqCst);
            slint::quit_event_loop().ok();
        });
    }

    let weak = ui.as_weak();
    let loop_code = exit_code.clone();
    std::thread::spawn(move || {
        runner::run_lifecycle(cfg, move |event| {
            let weak = weak.clone();
            let loop_code = loop_code.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                match event {
                    runner::UiEvent::HookStarted { name } => {
                        ui.set_status_text(hook_status(name).into());
                    }
                    runner::UiEvent::OutputLine { line } => {
                        push_output(&ui, &line);
                    }
                    runner::UiEvent::ProvisionStarted { file, total } => {
                        match total {
                            Some(t) => {
                                push_output(&ui, &format!("正在下载 {file}（共 {t} 字节）…"))
                            }
                            None => push_output(&ui, &format!("正在下载 {file}…")),
                        }
                        ui.set_status_text(format!("正在下载 {file}…").into());
                        ui.set_show_progress(true);
                        ui.set_provision_file(file.into());
                        ui.set_provision_progress(match total {
                            Some(_) => 0.0,
                            None => -1.0,
                        });
                        ui.set_provision_detail("".into());
                    }
                    runner::UiEvent::ProvisionProgress {
                        file,
                        downloaded,
                        total,
                        bps,
                    } => {
                        let ratio = match total {
                            Some(t) if t > 0 => downloaded as f32 / t as f32,
                            _ => -1.0,
                        };
                        ui.set_provision_file(file.into());
                        ui.set_provision_progress(ratio);
                        ui.set_provision_detail(
                            crate::python::format_detail(downloaded, total, bps).into(),
                        );
                    }
                    runner::UiEvent::ProvisionDone { file } => {
                        ui.set_show_progress(false);
                        ui.set_status_text("下载完成，正在准备…".into());
                        push_output(&ui, &format!("下载完成：{file}"));
                    }
                    runner::UiEvent::AppStarted => {
                        ui.hide().ok();
                    }
                    runner::UiEvent::AppExited { code } => {
                        ui.show().ok();
                        if code != 0 {
                            push_output(&ui, &format!("主程序已退出，退出码 {code}"));
                        }
                        ui.set_status_text("正在清理…".into());
                    }
                    runner::UiEvent::LifecycleDone { code } => {
                        loop_code.store(code, Ordering::SeqCst);
                        if code == 0 {
                            slint::quit_event_loop().ok();
                        } else {
                            ui.set_status_text(
                                format!("执行失败，退出码 {code}，详见上方输出与日志").into(),
                            );
                            ui.set_is_error(true);
                        }
                    }
                    runner::UiEvent::FatalError { message } => {
                        ui.set_status_text(message.into());
                        ui.set_is_error(true);
                    }
                }
            });
        });
    });

    if let Err(e) = ui.run() {
        eprintln!("界面运行失败：{e}");
        std::process::exit(2);
    }
    let code = exit_code.load(Ordering::SeqCst);
    std::process::exit(if code == EXIT_UNSET { 0 } else { code });
}

fn looks_like_path(s: &str) -> bool {
    s.contains('/') || s.contains('\\') || Path::new(s).is_absolute()
}

fn verify_files(cfg: &config::Config) -> Result<(), String> {
    let mut problems: Vec<String> = Vec::new();
    if let Some(logo) = cfg.splash.logo.as_deref()
        && looks_like_path(logo)
    {
        let path = cfg.exe_dir.join(logo);
        if !path.exists() {
            problems.push(format!("[splash] logo 不存在：{}", path.display()));
        }
    }
    for (name, hook) in [
        ("check_update", &cfg.hooks.check_update),
        ("install", &cfg.hooks.install),
        ("start", &cfg.hooks.start),
        ("end", &cfg.hooks.end),
    ] {
        if let Some(argv) = hook {
            if argv.is_empty() {
                continue;
            }
            match cfg.resolve_program(&argv[0]) {
                Ok(program) => {
                    let text = program.to_string_lossy();
                    if looks_like_path(&text) && !Path::new(program.as_os_str()).exists() {
                        problems.push(format!("[hooks] {name} 程序不存在：{text}"));
                    }
                }
                Err(e) => problems.push(format!("[hooks] {name} 解析失败：{e}")),
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn cli_config_or_default(cli_config: &Option<PathBuf>) -> String {
    match cli_config {
        Some(path) => path.display().to_string(),
        None => "默认位置".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_bundle_resolves_to_resources() {
        let exe = Path::new("/Applications/MyApp.app/Contents/MacOS/MyApp");
        assert_eq!(
            resolve_exe_dir(exe),
            PathBuf::from("/Applications/MyApp.app/Contents/Resources")
        );
    }

    #[test]
    fn plain_binary_resolves_to_own_dir() {
        let exe = Path::new("/opt/dist/compressed-towel");
        assert_eq!(resolve_exe_dir(exe), PathBuf::from("/opt/dist"));
    }

    #[test]
    fn lookalike_path_without_app_suffix_stays_plain() {
        let exe = Path::new("/opt/NotApp/Contents/MacOS/tool");
        assert_eq!(
            resolve_exe_dir(exe),
            PathBuf::from("/opt/NotApp/Contents/MacOS")
        );
    }
}
