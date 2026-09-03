use std::path::Path;
use std::process::Command;

pub fn run(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").current_dir(cwd).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The committed version of a file, or None outside a repo / for new files.
pub fn show_head(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    let root = run(dir, &["rev-parse", "--show-toplevel"])?;
    let root = Path::new(root.trim());
    let rel = path.strip_prefix(root).ok()?;
    run(root, &["show", &format!("HEAD:{}", rel.display())])
}
