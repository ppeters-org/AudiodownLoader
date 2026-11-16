
use std::path::PathBuf;

pub fn extend_path() {
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();

    let mut extra: Vec<PathBuf> = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    // Python.org framework installs, e.g. .../Versions/3.12/bin
    if let Ok(entries) = std::fs::read_dir("/Library/Frameworks/Python.framework/Versions") {
        for e in entries.flatten() {
            extra.push(e.path().join("bin"));
        }
    }
    // pip --user installs
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        extra.push(home.join(".local/bin"));
    }

    for p in extra {
        if p.is_dir() && !paths.contains(&p) {
            paths.push(p);
        }
    }
    if let Ok(new_path) = std::env::join_paths(paths) {
        // If your crate uses edition 2024, wrap this call in `unsafe { }`.
        std::env::set_var("PATH", new_path);
    }
}
