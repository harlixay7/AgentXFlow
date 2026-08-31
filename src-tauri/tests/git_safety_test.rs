use std::path::Path;
use std::process::Command;

pub fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git must run");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "t@t.t"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("a.txt"), "v1").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "init"]);
}

pub fn setup_repo_with_changes(dir: &Path, staged: bool, untracked: bool) {
    init_repo(dir);
    std::fs::write(dir.join("a.txt"), "v2-user").unwrap(); // unstaged edit
    if staged {
        git(dir, &["add", "a.txt"]);
    }
    if untracked {
        std::fs::write(dir.join("user-note.txt"), "keep me").unwrap();
    }
}
