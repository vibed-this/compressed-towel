use std::path::PathBuf;

fn main() {
    slint_build::compile("ui/app.slint").expect("Slint compilation failed");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=ui/app.slint");
    println!("cargo:rerun-if-changed=launcher.template.toml");

    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let scripts_dir = manifest_dir.join("templates").join("scripts");
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut stack = vec![scripts_dir.clone()];
    while let Some(dir) = stack.pop() {
        let mut children: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("templates/scripts is readable") {
            let path = entry.expect("dir entry is readable").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                children.push(path);
            }
        }
        children.sort();
        for path in children {
            let key = path
                .strip_prefix(manifest_dir.join("templates"))
                .expect("script is under templates/")
                .to_string_lossy()
                .replace('\\', "/");
            println!("cargo:rerun-if-changed=templates/{key}");
            entries.push((key, path.to_string_lossy().into_owned()));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::from("pub const EMBEDDED_FILES: &[(&str, &str)] = &[\n");
    for (key, abs) in &entries {
        out.push_str(&format!("    ({key:?}, include_str!({abs:?})),\n"));
    }
    out.push_str("];\n");
    let out_path = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set"))
        .join("embedded_manifest.rs");
    std::fs::write(out_path, out).expect("embedded manifest is writable");
}
