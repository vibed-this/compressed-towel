use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub enum ConfigError {
    Io { file: PathBuf, message: String },
    Parse { file: PathBuf, message: String },
    Missing { file: PathBuf, message: String },
    Invalid { file: PathBuf, message: String },
    Confinement { file: PathBuf, message: String },
    UserOverlay { file: PathBuf, message: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { file, message } => {
                write!(f, "{}: {}", file.display(), message)
            }
            Self::Parse { file, message } => {
                write!(f, "{}: {}", file.display(), message)
            }
            Self::Missing { file, message } => {
                write!(f, "{}: {}", file.display(), message)
            }
            Self::Invalid { file, message } => {
                write!(f, "{}: {}", file.display(), message)
            }
            Self::Confinement { file, message } => {
                write!(f, "{}: {}", file.display(), message)
            }
            Self::UserOverlay { file, message } => {
                write!(f, "{}: {}", file.display(), message)
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub name: String,
    pub version: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            name: "CompressedTowel".to_string(),
            version: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SplashConfig {
    pub logo: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Hooks {
    pub check_update: Option<Vec<String>>,
    pub install: Option<Vec<String>>,
    pub start: Option<Vec<String>>,
    pub end: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct EnvConfig {
    pub vars: Vec<(String, String)>,
    pub path_prepend: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LauncherConfig {
    pub log_level: String,
    pub user_editable: Vec<String>,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            user_editable: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub app: AppConfig,
    pub splash: SplashConfig,
    pub hooks: Hooks,
    pub env: EnvConfig,
    pub resolved_env: HashMap<String, String>,
    pub launcher: LauncherConfig,
    pub exe_dir: PathBuf,
    pub log_dir: PathBuf,
    /// 内建脚本解压目录（外部同名文件缺失时回退）；无回退需求时为 None。
    pub embedded_dir: Option<PathBuf>,
}

fn parse_toml_file(path: &Path) -> Result<toml::Value, ConfigError> {
    let text = fs::read_to_string(path).map_err(|e| ConfigError::Io {
        file: path.to_path_buf(),
        message: format!("cannot read file: {e}"),
    })?;
    toml::from_str::<toml::Value>(&text).map_err(|e| ConfigError::Parse {
        file: path.to_path_buf(),
        message: format!("toml parse error: {e}"),
    })
}

fn expand_process_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let rest = &input[i..];
        if rest.starts_with("${") {
            match rest.find('}') {
                Some(end) => {
                    let name = &rest[2..end];
                    let val = std::env::var(name).unwrap_or_default();
                    out.push_str(&val);
                    i += end + 1;
                }
                None => {
                    out.push_str(rest);
                    break;
                }
            }
        } else {
            let ch = rest.chars().next().unwrap_or('\0');
            if ch == '\0' {
                break;
            }
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn expand_with_resolved(input: &str, resolved: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let rest = &input[i..];
        if rest.starts_with("${") {
            match rest.find('}') {
                Some(end) => {
                    let name = &rest[2..end];
                    if let Some(val) = resolved.get(name) {
                        out.push_str(val);
                    } else {
                        out.push_str(&std::env::var(name).unwrap_or_default());
                    }
                    i += end + 1;
                }
                None => {
                    out.push_str(rest);
                    break;
                }
            }
        } else {
            let ch = rest.chars().next().unwrap_or('\0');
            if ch == '\0' {
                break;
            }
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn is_bare_command(raw: &str) -> bool {
    if raw.is_empty() || raw.contains('/') || raw.contains('\\') {
        return false;
    }
    let mut comps = Path::new(raw).components();
    matches!(
        (comps.next(), comps.next()),
        (Some(Component::Normal(_)), None)
    )
}

fn normalize_lexically(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            _ => out.push(c.as_os_str()),
        }
    }
    out
}

fn canonical_confined(exe_canonical: &Path, normalized_joined: &Path) -> Option<bool> {
    let mut suffix: Vec<OsString> = Vec::new();
    let mut cur: &Path = normalized_joined;
    loop {
        match cur.canonicalize() {
            Ok(c) => {
                let mut rebuilt = c;
                for s in suffix.iter().rev() {
                    rebuilt.push(s);
                }
                return Some(normalize_lexically(&rebuilt).starts_with(exe_canonical));
            }
            Err(_) => match cur.file_name() {
                Some(f) => {
                    suffix.push(f.to_os_string());
                    match cur.parent() {
                        Some(p) => cur = p,
                        None => return None,
                    }
                }
                None => return None,
            },
        }
    }
}

fn confine_joined(
    exe_dir: &Path,
    expanded: &str,
    bare_trusted: bool,
    file_ctx: &Path,
    kind: &str,
) -> Result<PathBuf, ConfigError> {
    if expanded.is_empty() {
        return Err(ConfigError::Invalid {
            file: file_ctx.to_path_buf(),
            message: format!("{kind} is empty"),
        });
    }
    let p = Path::new(expanded);
    if p.is_absolute() {
        return Ok(p.to_path_buf());
    }
    if bare_trusted && is_bare_command(expanded) {
        return Ok(p.to_path_buf());
    }
    let joined = exe_dir.join(p);
    let norm_exe = normalize_lexically(exe_dir);
    let norm_joined = normalize_lexically(&joined);
    if !norm_joined.starts_with(&norm_exe) {
        return Err(ConfigError::Confinement {
            file: file_ctx.to_path_buf(),
            message: format!("{kind} '{expanded}' escapes exe_dir"),
        });
    }
    if let Ok(exe_canonical) = exe_dir.canonicalize()
        && let Some(false) = canonical_confined(&exe_canonical, &norm_joined)
    {
        return Err(ConfigError::Confinement {
            file: file_ctx.to_path_buf(),
            message: format!("{kind} '{expanded}' escapes exe_dir"),
        });
    }
    Ok(norm_joined)
}

fn collect_leaves(value: &toml::Value, prefix: String, out: &mut Vec<String>) {
    match value {
        toml::Value::Table(t) => {
            for (k, v) in t {
                let next = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                collect_leaves(v, next, out);
            }
        }
        _ => {
            if !prefix.is_empty() {
                out.push(prefix);
            }
        }
    }
}

fn deep_merge(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(existing) => deep_merge(existing, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

fn section_table<'a>(
    root: &'a toml::Table,
    section: &str,
    file: &Path,
) -> Result<Option<&'a toml::Table>, ConfigError> {
    match root.get(section) {
        None => Ok(None),
        Some(toml::Value::Table(t)) => Ok(Some(t)),
        Some(_) => Err(ConfigError::Invalid {
            file: file.to_path_buf(),
            message: format!("[{section}] must be a table"),
        }),
    }
}

fn get_opt_string(
    table: &toml::Table,
    section: &str,
    key: &str,
    file: &Path,
) -> Result<Option<String>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ConfigError::Invalid {
            file: file.to_path_buf(),
            message: format!("[{section}] '{key}' must be a string"),
        }),
    }
}

fn get_hook_array(
    table: &toml::Table,
    key: &str,
    file: &Path,
) -> Result<Option<Vec<String>>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    toml::Value::String(s) => out.push(s.clone()),
                    _ => {
                        return Err(ConfigError::Invalid {
                            file: file.to_path_buf(),
                            message: format!("[hooks] '{key}' must be an array of strings"),
                        });
                    }
                }
            }
            Ok(Some(out))
        }
        Some(_) => Err(ConfigError::Invalid {
            file: file.to_path_buf(),
            message: format!("[hooks] '{key}' must be an array of strings, found non-array"),
        }),
    }
}

fn get_string_array(
    value: Option<&toml::Value>,
    section: &str,
    key: &str,
    file: &Path,
) -> Result<Vec<String>, ConfigError> {
    match value {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    toml::Value::String(s) => out.push(s.clone()),
                    _ => {
                        return Err(ConfigError::Invalid {
                            file: file.to_path_buf(),
                            message: format!("[{section}] '{key}' must be an array of strings"),
                        });
                    }
                }
            }
            Ok(out)
        }
        Some(_) => Err(ConfigError::Invalid {
            file: file.to_path_buf(),
            message: format!("[{section}] '{key}' must be an array of strings, found non-array"),
        }),
    }
}

fn extract_whitelist(root: &toml::Table, file: &Path) -> Result<Vec<String>, ConfigError> {
    let launcher = section_table(root, "launcher", file)?
        .cloned()
        .unwrap_or_default();
    get_string_array(
        launcher.get("user_editable"),
        "launcher",
        "user_editable",
        file,
    )
}

fn expand_hook_argv(argv: &[String], resolved: &HashMap<String, String>) -> Vec<String> {
    argv.iter()
        .map(|a| expand_with_resolved(a, resolved))
        .collect()
}

fn set_dotted(root: &mut toml::Value, leaf: &str, val: String) {
    let mut parts = leaf.split('.').collect::<Vec<_>>();
    let Some(last) = parts.pop() else { return };
    let mut cur = root;
    for part in parts {
        let is_table = cur.get(part).is_some_and(|v| v.is_table());
        if !is_table {
            let Some(table) = cur.as_table_mut() else {
                return;
            };
            table.insert(part.to_string(), toml::Value::Table(toml::map::Map::new()));
        }
        let Some(next) = cur.get_mut(part) else {
            return;
        };
        cur = next;
    }
    if let Some(table) = cur.as_table_mut() {
        table.insert(last.to_string(), toml::Value::String(val));
    }
}

const ENV_OVERRIDES: [(&str, &str); 3] = [
    ("app.name", "CT_APP_NAME"),
    ("splash.logo", "CT_SPLASH_LOGO"),
    ("launcher.log_level", "CT_LAUNCHER_LOG_LEVEL"),
];

fn apply_env_overrides(merged: &mut toml::Value) {
    for (leaf, env_name) in ENV_OVERRIDES {
        if let Ok(val) = std::env::var(env_name) {
            set_dotted(merged, leaf, val);
        }
    }
}

fn assemble(
    exe_dir: &Path,
    main_path: &Path,
    mut merged: toml::Value,
    user_overlay: Option<(PathBuf, toml::Value)>,
) -> Result<Config, ConfigError> {
    let root = merged.as_table().ok_or_else(|| ConfigError::Invalid {
        file: main_path.to_path_buf(),
        message: "config root must be a table".to_string(),
    })?;
    let whitelist = extract_whitelist(root, main_path)?;
    let allowed: BTreeSet<&str> = whitelist.iter().map(String::as_str).collect();

    if let Some((user_path, user_val)) = user_overlay {
        let mut leaves = Vec::new();
        collect_leaves(&user_val, String::new(), &mut leaves);
        for leaf in &leaves {
            if !allowed.contains(leaf.as_str()) {
                return Err(ConfigError::UserOverlay {
                    file: user_path,
                    message: format!("key '{leaf}' is not in launcher.user_editable whitelist"),
                });
            }
        }
        let base = merged.as_table_mut().ok_or_else(|| ConfigError::Invalid {
            file: main_path.to_path_buf(),
            message: "config root must be a table".to_string(),
        })?;
        let _ = base;
        deep_merge(&mut merged, user_val);
    }
    apply_env_overrides(&mut merged);

    let root = merged.as_table().ok_or_else(|| ConfigError::Invalid {
        file: main_path.to_path_buf(),
        message: "config root must be a table".to_string(),
    })?;

    let app_name = match section_table(root, "app", main_path)? {
        None => "CompressedTowel".to_string(),
        Some(t) => get_opt_string(t, "app", "name", main_path)?
            .unwrap_or_else(|| "CompressedTowel".to_string()),
    };
    let app_version = match section_table(root, "app", main_path)? {
        None => None,
        Some(t) => get_opt_string(t, "app", "version", main_path)?,
    };
    let app = AppConfig {
        name: app_name.clone(),
        version: app_version,
    };

    let splash_raw = match section_table(root, "splash", main_path)? {
        None => SplashConfig { logo: None },
        Some(t) => SplashConfig {
            logo: get_opt_string(t, "splash", "logo", main_path)?,
        },
    };

    let hooks_table = section_table(root, "hooks", main_path)?
        .cloned()
        .unwrap_or_default();
    let hooks_raw = Hooks {
        check_update: get_hook_array(&hooks_table, "check_update", main_path)?,
        install: get_hook_array(&hooks_table, "install", main_path)?,
        start: get_hook_array(&hooks_table, "start", main_path)?,
        end: get_hook_array(&hooks_table, "end", main_path)?,
    };

    let env_cfg_raw = match section_table(root, "env", main_path)? {
        None => EnvConfig::default(),
        Some(t) => {
            let mut vars = Vec::new();
            let mut path_prepend = Vec::new();
            for (k, v) in t {
                if k == "PATH" {
                    match v {
                        toml::Value::Table(pt) => {
                            for (pk, pv) in pt {
                                if pk != "prepend" {
                                    return Err(ConfigError::Invalid {
                                        file: main_path.to_path_buf(),
                                        message: format!(
                                            "[env.PATH] unknown key '{pk}', only 'prepend' is allowed"
                                        ),
                                    });
                                }
                                path_prepend =
                                    get_string_array(Some(pv), "env.PATH", "prepend", main_path)?;
                            }
                        }
                        _ => {
                            return Err(ConfigError::Invalid {
                                file: main_path.to_path_buf(),
                                message: "[env] 'PATH' must be a table with 'prepend'".to_string(),
                            });
                        }
                    }
                } else {
                    match v {
                        toml::Value::String(s) => vars.push((k.clone(), s.clone())),
                        _ => {
                            return Err(ConfigError::Invalid {
                                file: main_path.to_path_buf(),
                                message: format!("[env] '{k}' must be a string"),
                            });
                        }
                    }
                }
            }
            EnvConfig { vars, path_prepend }
        }
    };

    let launcher_raw = match section_table(root, "launcher", main_path)? {
        None => LauncherConfig::default(),
        Some(t) => LauncherConfig {
            log_level: get_opt_string(t, "launcher", "log_level", main_path)?
                .unwrap_or_else(|| "info".to_string()),
            user_editable: get_string_array(
                t.get("user_editable"),
                "launcher",
                "user_editable",
                main_path,
            )?,
        },
    };

    let mut resolved_env: HashMap<String, String> = HashMap::with_capacity(env_cfg_raw.vars.len());
    let mut env_vars: Vec<(String, String)> = Vec::with_capacity(env_cfg_raw.vars.len());
    for (k, v) in &env_cfg_raw.vars {
        let expanded = expand_process_env(v);
        resolved_env.insert(k.clone(), expanded.clone());
        env_vars.push((k.clone(), expanded));
    }

    let splash = SplashConfig {
        logo: splash_raw
            .logo
            .as_deref()
            .map(|s| expand_with_resolved(s, &resolved_env)),
    };

    let hooks = Hooks {
        check_update: hooks_raw
            .check_update
            .as_deref()
            .map(|a| expand_hook_argv(a, &resolved_env)),
        install: hooks_raw
            .install
            .as_deref()
            .map(|a| expand_hook_argv(a, &resolved_env)),
        start: hooks_raw
            .start
            .as_deref()
            .map(|a| expand_hook_argv(a, &resolved_env)),
        end: hooks_raw
            .end
            .as_deref()
            .map(|a| expand_hook_argv(a, &resolved_env)),
    };

    match &hooks.start {
        Some(v) if !v.is_empty() => {}
        _ => {
            return Err(ConfigError::Missing {
                file: main_path.to_path_buf(),
                message: "[hooks] start is required".to_string(),
            });
        }
    }

    let env = EnvConfig {
        vars: env_vars,
        path_prepend: env_cfg_raw
            .path_prepend
            .iter()
            .map(|s| expand_with_resolved(s, &resolved_env))
            .collect(),
    };

    if let Some(logo) = &splash.logo {
        confine_joined(exe_dir, logo, false, main_path, "[splash] logo")?;
    }
    for (name, hook) in [
        ("check_update", &hooks.check_update),
        ("install", &hooks.install),
        ("start", &hooks.start),
        ("end", &hooks.end),
    ] {
        if let Some(argv) = hook
            && let Some(program) = argv.first()
            && !falls_back_to_embedded(exe_dir, program)
        {
            confine_joined(
                exe_dir,
                program,
                true,
                main_path,
                &format!("[hooks] {name} program"),
            )?;
        }
    }
    for entry in &env.path_prepend {
        confine_joined(exe_dir, entry, false, main_path, "[env.PATH] prepend")?;
    }

    let data_dir = dirs::data_dir().ok_or_else(|| ConfigError::Missing {
        file: main_path.to_path_buf(),
        message: "cannot determine data dir for log_dir".to_string(),
    })?;
    let log_dir = data_dir.join(&app.name).join("logs");

    Ok(Config {
        app,
        splash,
        hooks,
        env,
        resolved_env,
        launcher: launcher_raw,
        exe_dir: exe_dir.to_path_buf(),
        log_dir,
        embedded_dir: None,
    })
}

impl Config {
    pub fn load(exe_dir: &Path, cli_config: Option<&Path>) -> Result<Config, ConfigError> {
        let main_path = match cli_config {
            Some(p) => p.to_path_buf(),
            None => exe_dir.join("launcher.toml"),
        };
        let main_val = match parse_toml_file(&main_path) {
            Ok(v) => v,
            Err(e) => {
                let missing_default = cli_config.is_none() && matches!(e, ConfigError::Io { .. });
                if !missing_default {
                    return Err(e);
                }
                toml::from_str::<toml::Value>(crate::embedded::DEFAULT_LAUNCHER_TOML).map_err(
                    |e| ConfigError::Parse {
                        file: main_path.clone(),
                        message: format!("built-in default config is broken: {e}"),
                    },
                )?
            }
        };
        let app_name = main_val
            .as_table()
            .and_then(|t| t.get("app"))
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("CompressedTowel")
            .to_string();
        let user_path = dirs::data_dir().map(|d| d.join(&app_name).join("user.toml"));
        let user_overlay = match user_path {
            Some(p) if p.is_file() => {
                let v = parse_toml_file(&p)?;
                Some((p, v))
            }
            _ => None,
        };
        assemble(exe_dir, &main_path, main_val, user_overlay).and_then(|mut cfg| {
            cfg.embedded_dir =
                materialize_embedded(exe_dir, &main_path, &cfg.app.name, &cfg.hooks)?;
            Ok(cfg)
        })
    }

    pub fn cleanup_embedded(&self) {
        if let Some(dir) = &self.embedded_dir {
            let _ = fs::remove_dir_all(dir);
        }
    }

    pub fn resolve_program(&self, raw: &str) -> Result<OsString, ConfigError> {
        let ctx = self.exe_dir.join("launcher.toml");
        let expanded = expand_with_resolved(raw, &self.resolved_env);
        if expanded.is_empty() {
            return Err(ConfigError::Invalid {
                file: ctx,
                message: "program is empty".to_string(),
            });
        }
        let p = Path::new(&expanded);
        if p.is_absolute() {
            return Ok(OsString::from(expanded));
        }
        if is_bare_command(&expanded) {
            return Ok(OsString::from(expanded));
        }
        let confined = confine_joined(&self.exe_dir, &expanded, false, &ctx, "program")?;
        if confined.exists() {
            return Ok(confined.into_os_string());
        }
        if crate::embedded::contains(&expanded)
            && let Some(dir) = &self.embedded_dir
        {
            let staged = dir.join(&expanded);
            if normalize_lexically(&staged).starts_with(normalize_lexically(dir))
                && staged.is_file()
            {
                return Ok(staged.into_os_string());
            }
        }
        Ok(confined.into_os_string())
    }

    pub fn build_env(&self) -> Vec<(OsString, OsString)> {
        let mut out = Vec::with_capacity(self.resolved_env.len() + 1);
        for (k, v) in &self.resolved_env {
            out.push((OsString::from(k), OsString::from(v)));
        }
        let mut prepend_abs: Vec<PathBuf> = Vec::new();
        let norm_exe = normalize_lexically(&self.exe_dir);
        for entry in &self.env.path_prepend {
            let expanded = expand_with_resolved(entry, &self.resolved_env);
            if expanded.is_empty() {
                continue;
            }
            let p = Path::new(&expanded);
            let abs = if p.is_absolute() {
                p.to_path_buf()
            } else {
                let joined = normalize_lexically(&self.exe_dir.join(p));
                if !joined.starts_with(&norm_exe) {
                    continue;
                }
                joined
            };
            prepend_abs.push(abs);
        }
        if !prepend_abs.is_empty() {
            let mut all = prepend_abs;
            if let Some(existing) = std::env::var_os("PATH") {
                all.extend(std::env::split_paths(&existing));
            }
            if let Ok(joined) = std::env::join_paths(all) {
                out.push((OsString::from("PATH"), joined));
            }
        }
        out
    }
}

/// 内建回退判定：相对路径、外部同名文件缺失、且内建清单里有 →
// 跳过外部存在性校验，运行时用解压副本。
fn falls_back_to_embedded(exe_dir: &Path, program: &str) -> bool {
    let p = Path::new(program);
    if p.is_absolute() || is_bare_command(program) {
        return false;
    }
    !exe_dir.join(p).exists() && crate::embedded::contains(program)
}

const EMBEDDED_DIR_PREFIX: &str = "ct-embedded-";
const EMBEDDED_STALE_AFTER_SECS: u64 = 24 * 60 * 60;

fn sanitize_app_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() {
        "app".to_string()
    } else {
        clean
    }
}

/// 收集需要内建回退的脚本名（去重，保持 hook 出现顺序）。
fn embedded_needed(exe_dir: &Path, hooks: &Hooks) -> Vec<String> {
    let mut out = Vec::new();
    for hook in [
        &hooks.check_update,
        &hooks.install,
        &hooks.start,
        &hooks.end,
    ] {
        if let Some(argv) = hook
            && let Some(program) = argv.first()
            && falls_back_to_embedded(exe_dir, program)
            && !out.contains(program)
        {
            out.push(program.clone());
        }
    }
    out
}

/// 清理 crash 遗留的解压目录：只删同应用前缀且 mtime 超过一天的；
// 当轮与并发实例的目录都很新，不会被误删。
fn sweep_stale_embedded(root: &Path, sanitized: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let prefix = format!("{EMBEDDED_DIR_PREFIX}{sanitized}-");
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age.as_secs() > EMBEDDED_STALE_AFTER_SECS);
        if stale {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// 解压本轮需要的内建脚本到临时目录；无回退需求时返回 None，不碰磁盘。
fn materialize_embedded(
    exe_dir: &Path,
    ctx: &Path,
    app_name: &str,
    hooks: &Hooks,
) -> Result<Option<PathBuf>, ConfigError> {
    let needed = embedded_needed(exe_dir, hooks);
    if needed.is_empty() {
        return Ok(None);
    }
    let sanitized = sanitize_app_name(app_name);
    let root = std::env::temp_dir();
    sweep_stale_embedded(&root, &sanitized);
    let dir = root.join(format!(
        "{EMBEDDED_DIR_PREFIX}{sanitized}-{}",
        std::process::id()
    ));
    let io_err = |e: std::io::Error| ConfigError::Io {
        file: ctx.to_path_buf(),
        message: format!("embedded staging failed: {e}"),
    };
    fs::create_dir_all(&dir).map_err(io_err)?;
    for name in &needed {
        let content = crate::embedded::get(name).ok_or_else(|| ConfigError::Invalid {
            file: ctx.to_path_buf(),
            message: format!("embedded script vanished: {name}"),
        })?;
        let dest = dir.join(name);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
        fs::write(&dest, content).map_err(io_err)?;
    }
    Ok(Some(dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_exe_dir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("ct_cfg_{}_{}_{}", std::process::id(), tag, n));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn load_with_files(
        exe_dir: &Path,
        main_path: &Path,
        user_path: Option<&Path>,
    ) -> Result<Config, ConfigError> {
        let main_val = parse_toml_file(main_path)?;
        let user_overlay = match user_path {
            Some(p) => Some((p.to_path_buf(), parse_toml_file(p)?)),
            None => None,
        };
        assemble(exe_dir, main_path, main_val, user_overlay)
    }

    #[test]
    fn layered_override_applies_whitelisted_key() {
        let exe = temp_exe_dir("layer");
        let main_path = exe.join("launcher.toml");
        write_file(
            &main_path,
            "[app]\nname = \"LayerApp\"\n[hooks]\nstart = [\"run\"]\n[launcher]\nlog_level = \"info\"\nuser_editable = [\"launcher.log_level\"]\n",
        );
        let user_path = exe.join("user.toml");
        write_file(&user_path, "[launcher]\nlog_level = \"debug\"\n");
        let cfg = load_with_files(&exe, &main_path, Some(&user_path)).unwrap();
        assert_eq!(cfg.launcher.log_level, "debug");
        assert_eq!(cfg.app.name, "LayerApp");
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn env_vars_expand() {
        unsafe {
            std::env::set_var("CT_TEST_VAR_X", "hello");
        }
        let exe = temp_exe_dir("expand");
        let main_path = exe.join("launcher.toml");
        write_file(
            &main_path,
            "[app]\nname = \"ExpandApp\"\n[hooks]\nstart = [\"run-${CT_TEST_VAR_X}\"]\n[env]\nGREETING = \"hi-${CT_TEST_VAR_X}\"\n[launcher]\nlog_level = \"info\"\n",
        );
        let cfg = load_with_files(&exe, &main_path, None).unwrap();
        let start = cfg.hooks.start.clone().unwrap();
        assert_eq!(start[0], "run-hello");
        assert!(
            cfg.env
                .vars
                .contains(&("GREETING".to_string(), "hi-hello".to_string()))
        );
        assert_eq!(
            cfg.resolved_env.get("GREETING").map(String::as_str),
            Some("hi-hello")
        );
        let env = cfg.build_env();
        assert!(
            env.iter()
                .any(|(k, v)| k == "GREETING" && v == &OsString::from("hi-hello"))
        );
        unsafe {
            std::env::remove_var("CT_TEST_VAR_X");
        }
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn confinement_rejects_escape() {
        let exe = temp_exe_dir("confine");
        let main_path = exe.join("launcher.toml");
        write_file(
            &main_path,
            "[app]\nname = \"ConfineApp\"\n[hooks]\nstart = [\"../evil/run\"]\n",
        );
        let err = load_with_files(&exe, &main_path, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("launcher.toml"));
        assert!(msg.contains("evil") || msg.contains("escape"));
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn whitelist_rejects_unknown_key() {
        let exe = temp_exe_dir("white");
        let main_path = exe.join("launcher.toml");
        write_file(
            &main_path,
            "[app]\nname = \"WhiteApp\"\n[hooks]\nstart = [\"run\"]\n[launcher]\nlog_level = \"info\"\nuser_editable = [\"launcher.log_level\"]\n",
        );
        let user_path = exe.join("user.toml");
        write_file(&user_path, "[app]\nname = \"Other\"\n");
        let err = load_with_files(&exe, &main_path, Some(&user_path)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("user.toml"));
        assert!(msg.contains("app.name"));
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn env_override_wins_over_files() {
        unsafe {
            std::env::set_var("CT_LAUNCHER_LOG_LEVEL", "debug");
        }
        let exe = temp_exe_dir("envovr");
        let main_path = exe.join("launcher.toml");
        write_file(
            &main_path,
            "[app]\nname = \"EnvApp\"\n[hooks]\nstart = [\"run\"]\n[launcher]\nlog_level = \"info\"\n",
        );
        let cfg = load_with_files(&exe, &main_path, None).unwrap();
        assert_eq!(cfg.launcher.log_level, "debug");
        unsafe {
            std::env::remove_var("CT_LAUNCHER_LOG_LEVEL");
        }
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn hook_string_form_is_rejected() {
        let exe = temp_exe_dir("hookstr");
        let main_path = exe.join("launcher.toml");
        write_file(
            &main_path,
            "[app]\nname = \"HookStrApp\"\n[hooks]\nstart = \"run\"\n",
        );
        let err = load_with_files(&exe, &main_path, None).unwrap_err();
        assert!(err.to_string().contains("launcher.toml"));
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn env_runtime_expands_to_hook_and_resolves() {
        let exe = temp_exe_dir("rtenv");
        let main_path = exe.join("launcher.toml");
        write_file(
            &main_path,
            "[app]\nname = \"RtApp\"\n[env]\nRUNTIME = \"rtenv/tool\"\n[hooks]\nstart = [\"${RUNTIME}\", \"--help\"]\n",
        );
        let cfg = load_with_files(&exe, &main_path, None).unwrap();
        let start = cfg.hooks.start.clone().unwrap();
        assert_eq!(start[0], "rtenv/tool");
        let prog = cfg.resolve_program("${RUNTIME}").unwrap();
        assert_eq!(prog, exe.join("rtenv/tool").into_os_string());
        let prog2 = cfg.resolve_program(&start[0]).unwrap();
        assert_eq!(prog2, exe.join("rtenv/tool").into_os_string());
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn env_config_wins_over_process_env() {
        unsafe {
            std::env::set_var("CT_T_VAR_Q", "from-parent");
        }
        let exe = temp_exe_dir("cfgwin");
        let main_path = exe.join("launcher.toml");
        write_file(
            &main_path,
            "[app]\nname = \"CfgWinApp\"\n[env]\nCT_T_VAR_Q = \"from-config\"\n[hooks]\nstart = [\"${CT_T_VAR_Q}/run\"]\n",
        );
        let cfg = load_with_files(&exe, &main_path, None).unwrap();
        assert_eq!(
            cfg.resolved_env.get("CT_T_VAR_Q").map(String::as_str),
            Some("from-config")
        );
        let start = cfg.hooks.start.clone().unwrap();
        assert_eq!(start[0], "from-config/run");
        unsafe {
            std::env::remove_var("CT_T_VAR_Q");
        }
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn env_sibling_refs_do_not_expand() {
        unsafe {
            std::env::remove_var("CT_T_SIB_B_Q");
        }
        let exe = temp_exe_dir("sib");
        let main_path = exe.join("launcher.toml");
        write_file(
            &main_path,
            "[app]\nname = \"SibApp\"\n[env]\nCT_T_SIB_A_Q = \"${CT_T_SIB_B_Q}\"\n[hooks]\nstart = [\"run\"]\n",
        );
        let cfg = load_with_files(&exe, &main_path, None).unwrap();
        assert_eq!(
            cfg.resolved_env.get("CT_T_SIB_A_Q").map(String::as_str),
            Some("")
        );
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn missing_launcher_falls_back_to_builtin_default() {
        let exe = temp_exe_dir("builtin");
        let cfg = Config::load(&exe, None).expect("缺 launcher.toml 应回退内建默认");
        assert_eq!(cfg.app.name, "MyApp");
        assert!(cfg.hooks.start.is_some(), "内建默认应带 start");
        assert!(
            cfg.embedded_dir.is_some(),
            "内建默认引用的模板脚本外部缺失，应触发解压"
        );
        let prog = cfg
            .resolve_program("scripts/check_update.bat")
            .expect("内建脚本应可解析");
        let staged = cfg.embedded_dir.clone().expect("上面已断言为 Some");
        assert!(
            prog.to_string_lossy()
                .starts_with(&staged.to_string_lossy().into_owned()),
            "应解析到解压目录"
        );
        cfg.cleanup_embedded();
        assert!(!staged.exists(), "清理后解压目录应消失");
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn external_file_wins_over_embedded() {
        let exe = temp_exe_dir("extwin");
        write_file(&exe.join("scripts/check_update.bat"), "@echo external");
        let main_path = exe.join("launcher.toml");
        write_file(
            &main_path,
            "[app]\nname = \"ExtWinApp\"\n[hooks]\nstart = [\"scripts/check_update.bat\"]\n",
        );
        let cfg = load_with_files(&exe, &main_path, None).unwrap();
        let prog = cfg.resolve_program("scripts/check_update.bat").unwrap();
        assert_eq!(prog, exe.join("scripts/check_update.bat").into_os_string());
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn materialize_writes_embedded_content() {
        let exe = temp_exe_dir("matwrite");
        let hooks = Hooks {
            check_update: Some(vec!["scripts/install.py".to_string()]),
            ..Hooks::default()
        };
        let dir = materialize_embedded(&exe, &exe.join("launcher.toml"), "MatApp", &hooks)
            .expect("解压应成功")
            .expect("有回退需求应返回目录");
        let disk = fs::read_to_string(dir.join("scripts/install.py")).unwrap();
        assert_eq!(disk, crate::embedded::get("scripts/install.py").unwrap());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&exe);
    }

    #[test]
    fn sweep_keeps_fresh_and_foreign_dirs() {
        let root = temp_exe_dir("sweep");
        let fresh = root.join("ct-embedded-SweepApp-999999");
        let foreign = root.join("something-else-entirely");
        fs::create_dir_all(&fresh).unwrap();
        fs::create_dir_all(&foreign).unwrap();
        sweep_stale_embedded(&root, "SweepApp");
        assert!(fresh.is_dir(), "很新的同前缀目录不应被清");
        assert!(foreign.is_dir(), "异前缀目录不应被碰");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sanitize_app_name_replaces_specials() {
        assert_eq!(sanitize_app_name("My App/1.0"), "My_App_1_0");
        assert_eq!(sanitize_app_name(""), "app");
    }
}
