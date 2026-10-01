//! 启动器生命周期执行器。
//!
//! 按 check_update → install → start → end 的顺序执行钩子命令，
//! 子进程输出按行收集为事件并追加写入日志文件。

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

/// 上报给界面层的生命周期事件。
#[derive(Debug, Clone)]
pub enum UiEvent {
    HookStarted {
        name: &'static str,
    },
    OutputLine {
        line: String,
    },
    ProvisionStarted {
        file: String,
        total: Option<u64>,
    },
    ProvisionProgress {
        file: String,
        downloaded: u64,
        total: Option<u64>,
        bps: u64,
    },
    ProvisionDone {
        file: String,
    },
    AppStarted,
    AppExited {
        code: i32,
    },
    LifecycleDone {
        code: i32,
    },
    FatalError {
        message: String,
    },
}

/// 执行完整生命周期并阻塞直到结束。
///
/// 所有事件经内部通道集中到单个分发线程再调用 `emit`，调用者看到的顺序与发送顺序一致。
pub fn run_lifecycle(
    config: crate::config::Config,
    emit: impl Fn(UiEvent) + Send + Sync + 'static,
) {
    let log_file = open_log_file(&config);
    let (tx, rx) = mpsc::channel::<UiEvent>();
    let dispatcher = std::thread::spawn(move || {
        let mut log_file = log_file;
        for event in rx {
            if let UiEvent::OutputLine { ref line } = event
                && let Some(file) = log_file.as_mut()
            {
                let _ = writeln!(file, "{line}");
            }
            emit(event);
        }
    });

    match drive(&config, &tx) {
        Outcome::Fatal(message) => {
            let _ = tx.send(UiEvent::FatalError { message });
        }
        Outcome::Done(code) => {
            let _ = tx.send(UiEvent::LifecycleDone { code });
        }
    }
    drop(tx);
    let _ = dispatcher.join();
    config.cleanup_embedded();
}

enum Outcome {
    Fatal(String),
    Done(i32),
}

enum HookResult {
    Ok,
    Failed(i32),
}

enum StartResult {
    SpawnFailed,
    Exited(i32),
}

fn drive(config: &crate::config::Config, tx: &Sender<UiEvent>) -> Outcome {
    if config.python.is_some()
        && let Err(message) = crate::python::ensure_python_stack(config, tx)
    {
        return Outcome::Fatal(message);
    }
    for (name, hook) in [
        ("check_update", &config.hooks.check_update),
        ("install", &config.hooks.install),
    ] {
        if let Some(argv) = hook {
            if argv.is_empty() {
                continue;
            }
            if let HookResult::Failed(code) = run_hook(config, tx, name, argv) {
                return Outcome::Done(code);
            }
        }
    }

    let start_argv: &[String] = match config.hooks.start.as_ref() {
        None => {
            return Outcome::Fatal("hooks.start 未配置，无法启动主程序".to_string());
        }
        Some(argv) if argv.is_empty() => {
            return Outcome::Fatal("hooks.start 为空，无法启动主程序".to_string());
        }
        Some(argv) => argv,
    };
    let start_code = match run_start(config, tx, start_argv) {
        StartResult::SpawnFailed => return Outcome::Done(1),
        StartResult::Exited(code) => code,
    };

    if let Some(argv) = config.hooks.end.as_ref()
        && !argv.is_empty()
        && let HookResult::Failed(code) = run_hook(config, tx, "end", argv)
    {
        return Outcome::Done(code);
    }
    Outcome::Done(start_code)
}

fn run_hook(
    config: &crate::config::Config,
    tx: &Sender<UiEvent>,
    name: &'static str,
    argv: &[String],
) -> HookResult {
    let _ = tx.send(UiEvent::HookStarted { name });
    match run_captured(config, argv, tx) {
        Some(0) => HookResult::Ok,
        Some(code) => HookResult::Failed(code),
        None => HookResult::Failed(1),
    }
}

/// 执行一条命令并把输出流式上报为事件，返回退出码；
/// `None` 表示解析或启动失败（供 python 供给复用同一执行路径）。
pub(crate) fn run_captured(
    config: &crate::config::Config,
    argv: &[String],
    tx: &Sender<UiEvent>,
) -> Option<i32> {
    let mut child = spawn_hook(config, argv)?;
    let readers = collect_output(&mut child, tx);
    let code = wait_code(&mut child);
    join_readers(readers);
    Some(code)
}

fn run_start(config: &crate::config::Config, tx: &Sender<UiEvent>, argv: &[String]) -> StartResult {
    let _ = tx.send(UiEvent::HookStarted { name: "start" });
    let mut child = match spawn_hook(config, argv) {
        Some(child) => child,
        None => return StartResult::SpawnFailed,
    };
    let _ = tx.send(UiEvent::AppStarted);
    let readers = collect_output(&mut child, tx);
    let code = wait_code(&mut child);
    join_readers(readers);
    let _ = tx.send(UiEvent::AppExited { code });
    StartResult::Exited(code)
}

fn spawn_hook(config: &crate::config::Config, argv: &[String]) -> Option<Child> {
    let raw = argv.first()?;
    let resolved: OsString = config.resolve_program(raw).ok()?;
    let mut cmd = build_command(resolved, &argv[1..]);
    cmd.current_dir(&config.exe_dir);
    cmd.envs(config.build_env());
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.spawn().ok()
}

fn build_command(resolved: OsString, args: &[String]) -> Command {
    let ext = Path::new(&resolved)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    match ext.as_deref() {
        Some("bat") | Some("cmd") => {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(&resolved).args(args);
            cmd
        }
        Some("ps1") => {
            let mut cmd = Command::new("powershell");
            cmd.arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(&resolved)
                .args(args);
            cmd
        }
        _ => {
            let mut cmd = Command::new(&resolved);
            cmd.args(args);
            cmd
        }
    }
}

fn collect_output(child: &mut Child, tx: &Sender<UiEvent>) -> [Option<JoinHandle<()>>; 2] {
    [
        child.stdout.take().map(|out| drain_to_events(out, tx)),
        child.stderr.take().map(|err| drain_to_events(err, tx)),
    ]
}

fn drain_to_events<R>(reader: R, tx: &Sender<UiEvent>) -> JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
{
    let tx = tx.clone();
    std::thread::spawn(move || {
        let mut buffered = BufReader::new(reader);
        let mut raw = Vec::new();
        loop {
            raw.clear();
            match buffered.read_until(b'\n', &mut raw) {
                Ok(0) => break,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&raw);
                    let line = text.trim_end_matches(['\r', '\n']);
                    let _ = tx.send(UiEvent::OutputLine {
                        line: line.to_owned(),
                    });
                }
                Err(_) => break,
            }
        }
    })
}

fn wait_code(child: &mut Child) -> i32 {
    child
        .wait()
        .map(|status| status.code().unwrap_or(1))
        .unwrap_or(1)
}

fn join_readers(readers: [Option<JoinHandle<()>>; 2]) {
    for reader in readers.into_iter().flatten() {
        let _ = reader.join();
    }
}

fn open_log_file(config: &crate::config::Config) -> Option<std::fs::File> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    std::fs::create_dir_all(&config.log_dir).ok()?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(config.log_dir.join(format!("{stamp}.log")))
        .ok()
}
