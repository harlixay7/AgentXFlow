use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInspectionResult {
    pub is_git_repo: bool,
    pub active_branch: Option<String>,
    pub remote_url: Option<String>,
    pub languages: Vec<String>,
    pub package_managers: Vec<String>,
    pub build_scripts: Vec<String>,
    pub test_scripts: Vec<String>,
    pub lint_scripts: Vec<String>,
    pub has_ci: bool,
    pub has_instruction_file: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitAncestorResult {
    Ancestor,
    NotAncestor,
}

#[derive(Debug, Clone, Default)]
pub struct GitService;

fn normalize_path_str(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let trimmed = s.trim();
    let without_unc = if let Some(stripped) = trimmed.strip_prefix("//?/") {
        stripped
    } else if let Some(stripped) = trimmed.strip_prefix(r"\\?\") {
        stripped
    } else {
        trimmed
    };
    #[cfg(target_os = "windows")]
    {
        without_unc.trim_end_matches('/').to_lowercase()
    }
    #[cfg(not(target_os = "windows"))]
    {
        without_unc.trim_end_matches('/').to_string()
    }
}

pub fn paths_match(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    if let (Ok(ca), Ok(cb)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        if ca == cb {
            return true;
        }
    }
    normalize_path_str(a) == normalize_path_str(b)
}

pub fn path_is_under_root(candidate: &Path, root: &Path) -> bool {
    // If candidate exists on disk, its filesystem canonical target must reside under canonical root.
    // A symlink inside root pointing outside root must be rejected!
    if let (Ok(cand_c), Ok(root_c)) = (
        std::fs::canonicalize(candidate),
        std::fs::canonicalize(root),
    ) {
        let cand_norm = normalize_path_str(&cand_c);
        let root_norm = normalize_path_str(&root_c);
        return cand_norm.starts_with(&root_norm)
            && (cand_norm.len() == root_norm.len()
                || cand_norm.as_bytes().get(root_norm.len()) == Some(&b'/'));
    }

    // For non-existent paths, if an ancestor exists, ensure that ancestor is under canonical root
    if let Ok(root_c) = std::fs::canonicalize(root) {
        let root_norm = normalize_path_str(&root_c);
        let mut curr = candidate;
        while let Some(parent) = curr.parent() {
            if parent.exists() {
                if let Ok(parent_c) = std::fs::canonicalize(parent) {
                    let parent_norm = normalize_path_str(&parent_c);
                    if !(parent_norm.starts_with(&root_norm)
                        && (parent_norm.len() == root_norm.len()
                            || parent_norm.as_bytes().get(root_norm.len()) == Some(&b'/')))
                    {
                        return false;
                    }
                }
                break;
            }
            curr = parent;
        }
    }

    let cand_norm = normalize_path_str(candidate);
    let root_norm = normalize_path_str(root);
    cand_norm.starts_with(&root_norm)
        && (cand_norm.len() == root_norm.len()
            || cand_norm.as_bytes().get(root_norm.len()) == Some(&b'/'))
}

fn kill_process_tree(child: &std::process::Child) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .output();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &format!("-{}", child.id())])
            .output();
    }
}

fn read_bounded<R: std::io::Read>(mut reader: R, max_bytes: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < max_bytes {
                    let to_take = std::cmp::min(n, max_bytes - buf.len());
                    buf.extend_from_slice(&chunk[..to_take]);
                }
            }
            Err(_) => break,
        }
    }
    buf
}

const MAX_GIT_OUTPUT_BYTES: usize = 1_048_576; // 1 MB retention per stream

impl GitService {
    pub fn new() -> Self {
        Self
    }

    pub fn run_git_cmd_raw(
        &self,
        cwd: &Path,
        args: &[&str],
    ) -> Result<(std::process::ExitStatus, String, String), String> {
        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env("GIT_TERMINAL_PROMPT", "0");
        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn git command {:?}: {}", args, e))?;

        let mut out_reader = child
            .stdout
            .take()
            .map(|handle| std::thread::spawn(move || read_bounded(handle, MAX_GIT_OUTPUT_BYTES)));
        let mut err_reader = child
            .stderr
            .take()
            .map(|handle| std::thread::spawn(move || read_bounded(handle, MAX_GIT_OUTPUT_BYTES)));

        let start = std::time::Instant::now();
        let timeout_secs = std::env::var("AGENTXFLOW_GIT_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60);
        let timeout = std::time::Duration::from_secs(timeout_secs);
        let mut timed_out = false;
        let mut exit_status: Option<std::process::ExitStatus> = None;

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    break;
                }
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        timed_out = true;
                        kill_process_tree(&child);
                        let _ = child.kill();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    kill_process_tree(&child);
                    let _ = child.kill();
                    return Err(format!("Error waiting for git command: {}", e));
                }
            }
        }
        let _ = child.wait();

        let stdout = out_reader
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        let stderr = err_reader
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();

        if timed_out {
            return Err(format!(
                "Git command {:?} timed out after {}s",
                args,
                timeout.as_secs()
            ));
        }

        if let Some(status) = exit_status {
            Ok((
                status,
                String::from_utf8_lossy(&stdout).to_string(),
                String::from_utf8_lossy(&stderr).to_string(),
            ))
        } else {
            Err("Git command terminated without exit status".to_string())
        }
    }

    pub fn run_git_cmd(&self, cwd: &Path, args: &[&str]) -> Result<String, String> {
        let (status, stdout, stderr) = self.run_git_cmd_raw(cwd, args)?;
        if status.success() {
            Ok(stdout.trim().to_string())
        } else {
            let err = stderr.trim().to_string();
            Err(if err.is_empty() {
                format!("Git command failed with status {}", status)
            } else {
                err
            })
        }
    }

    pub fn check_is_git_repo(&self, repo_path: &Path) -> Result<bool, String> {
        let (status, stdout, stderr) =
            self.run_git_cmd_raw(repo_path, &["rev-parse", "--is-inside-work-tree"])?;
        if status.success() {
            Ok(stdout.trim() == "true")
        } else {
            let err = stderr.trim();
            // Git rev-parse outputs "fatal: not a git repository (or any of the parent directories)" when legitimately not a repo
            if err.contains("not a git repository") || stdout.contains("not a git repository") {
                Ok(false)
            } else {
                Err(format!(
                    "Git error inspecting repository at {:?}: {}",
                    repo_path,
                    if err.is_empty() { stdout.trim() } else { err }
                ))
            }
        }
    }

    pub fn is_git_repo(&self, repo_path: &Path) -> bool {
        self.check_is_git_repo(repo_path).unwrap_or(false)
    }

    /// A path is "managed" by AgentXFlow only if it lives under a managed root AND is a
    /// registered git worktree of `repo_path` or follows the managed task directory convention.
    /// Managed roots are the coordinator worktree pool (data_dir/AgentXFlow/worktrees) and
    /// the legacy <repo>/.agentxflow directory.
    pub fn is_managed_worktree(
        &self,
        repo_path: &Path,
        managed_roots: &[PathBuf],
        candidate: &Path,
    ) -> bool {
        // Safety: candidate must never be the primary repository checkout or an ancestor of it
        if paths_match(candidate, repo_path) || repo_path.starts_with(candidate) {
            return false;
        }

        // Candidate must reside under at least one managed root
        let is_under_root = managed_roots
            .iter()
            .any(|root| path_is_under_root(candidate, root));
        if !is_under_root {
            return false;
        }

        // 1. Check if candidate is registered in git worktree list
        if let Ok(list) = self.run_git_cmd(repo_path, &["worktree", "list", "--porcelain"]) {
            let registered = list.lines().any(|l| {
                l.strip_prefix("worktree ")
                    .map(|p| paths_match(Path::new(p.trim()), candidate))
                    .unwrap_or(false)
            });
            if registered {
                return true;
            }
        }

        // 2. Fallback: leaf directory matching AgentXFlow conventions (task-<id> or integration)
        let leaf_name = candidate.file_name().and_then(|n| n.to_str()).unwrap_or("");
        leaf_name.starts_with("task-") || leaf_name == "integration"
    }

    pub fn init_repo(&self, repo_path: &Path) -> Result<(), String> {
        std::fs::create_dir_all(repo_path)
            .map_err(|e| format!("Failed to create folder {:?}: {}", repo_path, e))?;
        self.run_git_cmd(repo_path, &["init"])?;
        self.run_git_cmd(repo_path, &["checkout", "-b", "main"])
            .ok();
        self.run_git_cmd(repo_path, &["config", "user.name", "AgentXFlow"])
            .map_err(|e| format!("Failed to configure local git user.name: {}", e))?;
        self.run_git_cmd(repo_path, &["config", "user.email", "agentxflow@local"])
            .map_err(|e| format!("Failed to configure local git user.email: {}", e))?;

        let gitignore = repo_path.join(".gitignore");
        if !gitignore.exists() {
            let _ = std::fs::write(&gitignore, ".agentxflow/\n");
            self.run_git_cmd(repo_path, &["add", ".gitignore"]).ok();
        }

        if self.run_git_cmd(repo_path, &["rev-parse", "HEAD"]).is_err() {
            self.run_git_cmd(
                repo_path,
                &["commit", "--allow-empty", "-m", "Initial commit"],
            )
            .map_err(|e| format!("Failed to create initial commit: {}", e))?;
        }
        Ok(())
    }

    pub fn get_current_branch(&self, repo_path: &Path) -> Result<String, String> {
        self.run_git_cmd(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
    }

    pub fn get_head_sha(&self, repo_path: &Path) -> Result<String, String> {
        self.run_git_cmd(repo_path, &["rev-parse", "HEAD"])
    }

    pub fn get_ref_sha(&self, repo_path: &Path, ref_name: &str) -> Result<String, String> {
        self.run_git_cmd(repo_path, &["rev-parse", ref_name])
    }

    pub fn is_base_stale(
        &self,
        repo_path: &Path,
        base_sha: &str,
        target_branch: &str,
    ) -> Result<bool, String> {
        let current_target_sha = self.get_ref_sha(repo_path, target_branch)?;
        Ok(current_target_sha != base_sha)
    }

    pub fn is_ancestor(
        &self,
        repo_path: &Path,
        commit_sha: &str,
        branch_or_ref: &str,
    ) -> Result<GitAncestorResult, String> {
        let (status, stdout, stderr) = self.run_git_cmd_raw(
            repo_path,
            &["merge-base", "--is-ancestor", commit_sha, branch_or_ref],
        )?;

        match status.code() {
            Some(0) => Ok(GitAncestorResult::Ancestor),
            Some(1) => Ok(GitAncestorResult::NotAncestor),
            Some(code) => {
                let err = stderr.trim();
                let msg = if err.is_empty() { stdout.trim() } else { err };
                Err(format!(
                    "Git merge-base --is-ancestor failed (code {}): {}",
                    code, msg
                ))
            }
            None => Err(
                "Git merge-base --is-ancestor terminated without exit status (killed or timed out)"
                    .to_string(),
            ),
        }
    }

    pub fn create_worktree(
        &self,
        repo_path: &Path,
        worktree_dir: &Path,
        branch_name: &str,
        base_branch: &str,
    ) -> Result<PathBuf, String> {
        if let Some(parent) = worktree_dir.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create worktree parent directory: {}", e))?;
        }

        info!(
            "Creating git worktree at {:?} for branch {}",
            worktree_dir, branch_name
        );

        let branch_exists = self
            .run_git_cmd(repo_path, &["rev-parse", "--verify", branch_name])
            .is_ok();

        let mut args = vec!["worktree", "add"];
        let worktree_str = worktree_dir
            .to_str()
            .ok_or("Invalid UTF-8 in worktree path")?;

        if branch_exists {
            args.push(worktree_str);
            args.push(branch_name);
        } else {
            args.push("-b");
            args.push(branch_name);
            args.push(worktree_str);
            args.push(base_branch);
        }

        self.run_git_cmd(repo_path, &args)?;
        Ok(worktree_dir.to_path_buf())
    }

    /// Creates or recovers the dedicated hidden integration worktree at `.agentxflow/integration/<project-id>`
    pub fn ensure_integration_worktree(
        &self,
        repo_path: &Path,
        project_id: &str,
        target_branch: &str,
    ) -> Result<PathBuf, String> {
        let integration_dir = repo_path
            .join(".agentxflow")
            .join("integration")
            .join(project_id);

        if !integration_dir.exists() {
            std::fs::create_dir_all(&integration_dir).map_err(|e| e.to_string())?;
            let integration_str = integration_dir.to_str().ok_or("Invalid UTF-8 path")?;

            // Check if detached integration worktree can be added
            self.run_git_cmd(
                repo_path,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    integration_str,
                    target_branch,
                ],
            )?;
        }

        Ok(integration_dir)
    }

    pub fn remove_worktree(
        &self,
        repo_path: &Path,
        managed_roots: &[PathBuf],
        worktree_dir: &Path,
    ) -> Result<(), String> {
        if !self.is_managed_worktree(repo_path, managed_roots, worktree_dir) {
            warn!(
                "Refusing to remove unmanaged path {:?}: not a registered worktree of {:?}",
                worktree_dir, repo_path
            );
            return Ok(());
        }
        let worktree_str = worktree_dir
            .to_str()
            .ok_or("Invalid UTF-8 in worktree path")?;
        info!("Removing git worktree at {:?}", worktree_dir);

        if let Err(e) =
            self.run_git_cmd(repo_path, &["worktree", "remove", "--force", worktree_str])
        {
            error!(
                "Git worktree remove returned error: {}. Cleaning directory directly.",
                e
            );
        }

        self.run_git_cmd(repo_path, &["worktree", "prune"]).ok();

        if worktree_dir.exists() {
            let _ = Self::safe_remove_dir_all(worktree_dir);
        }

        Ok(())
    }

    #[allow(clippy::permissions_set_readonly_false)]
    pub fn safe_remove_dir_all(path: &Path) -> std::io::Result<()> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };

        let mut perms = metadata.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(path, perms);
        }

        if is_symlink_or_reparse_point(&metadata) {
            // Invariant 1: Never recursively traverse a symlink or Windows junction/reparse point.
            // Treat the entry as a leaf/link object and remove the link without touching the target.
            if std::fs::remove_file(path).is_err() {
                std::fs::remove_dir(path)?;
            }
            return Ok(());
        }

        if metadata.is_dir() {
            let entries = std::fs::read_dir(path)?;
            for entry in entries {
                let entry = entry?;
                Self::safe_remove_dir_all(&entry.path())?;
            }
            std::fs::remove_dir(path)
        } else {
            std::fs::remove_file(path)
        }
    }

    pub fn get_diff(
        &self,
        repo_path: &Path,
        base_ref: &str,
        target_ref: &str,
    ) -> Result<String, String> {
        self.run_git_cmd(
            repo_path,
            &["diff", &format!("{}...{}", base_ref, target_ref)],
        )
    }

    pub fn get_changed_files(
        &self,
        repo_path: &Path,
        base_ref: &str,
        target_ref: &str,
    ) -> Result<Vec<String>, String> {
        let output = self.run_git_cmd(
            repo_path,
            &[
                "diff",
                "--name-only",
                &format!("{}...{}", base_ref, target_ref),
            ],
        )?;
        let files = output
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(files)
    }

    pub fn get_worktree_head_sha(&self, worktree_dir: &Path) -> Result<String, String> {
        self.run_git_cmd(worktree_dir, &["rev-parse", "HEAD"])
    }

    pub fn check_worktree_cleanliness(&self, worktree_dir: &Path) -> Result<(), Vec<String>> {
        match self.run_git_cmd(worktree_dir, &["status", "--porcelain"]) {
            Ok(output) => {
                let lines: Vec<String> = output
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                if lines.is_empty() {
                    Ok(())
                } else {
                    Err(lines)
                }
            }
            Err(e) => Err(vec![format!("Failed to query worktree status: {}", e)]),
        }
    }

    pub fn get_worktree_mutations(
        &self,
        worktree_dir: &Path,
        base_sha: &str,
    ) -> Result<Vec<String>, String> {
        let mut changed_set = std::collections::HashSet::new();

        // 1. Committed diff between base_sha and worktree HEAD.
        //    Fail closed: if the diff cannot be computed the change set is unknown.
        let committed_output = self
            .run_git_cmd(worktree_dir, &["diff", "--name-only", base_sha, "HEAD"])
            .map_err(|e| {
                format!(
                    "Failed to compute committed diff against base '{}': {}",
                    base_sha, e
                )
            })?;
        for line in committed_output.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                changed_set.insert(trimmed.to_string());
            }
        }

        // 2. Uncommitted staged and unstaged changes.
        //    Fail closed: if the status cannot be queried the change set is unknown.
        let status_output = self
            .run_git_cmd(worktree_dir, &["status", "--porcelain"])
            .map_err(|e| format!("Failed to query worktree status for mutation audit: {}", e))?;
        for line in status_output.lines() {
            let trimmed = line.trim();
            if trimmed.len() > 3 {
                let rest = trimmed[3..].trim();
                if rest.contains("->") {
                    let parts: Vec<&str> = rest
                        .split("->")
                        .map(|s| s.trim().trim_matches('"'))
                        .collect();
                    for part in parts {
                        if !part.is_empty() {
                            changed_set.insert(part.to_string());
                        }
                    }
                } else {
                    let clean = rest.trim_matches('"');
                    if !clean.is_empty() {
                        changed_set.insert(clean.to_string());
                    }
                }
            }
        }

        let mut list: Vec<String> = changed_set.into_iter().collect();
        list.sort();
        Ok(list)
    }

    pub fn check_worktree_health(&self, worktree_dir: &Path) -> bool {
        if !worktree_dir.exists() {
            return false;
        }
        self.run_git_cmd(worktree_dir, &["status"]).is_ok()
    }

    /// Auto-inspects repository structure for the V2 Import Wizard
    pub fn inspect_repository(&self, repo_path: &Path) -> RepoInspectionResult {
        let is_git = self.is_git_repo(repo_path);
        let active_branch = if is_git {
            self.get_current_branch(repo_path).ok()
        } else {
            None
        };
        let remote_url = if is_git {
            self.run_git_cmd(repo_path, &["remote", "get-url", "origin"])
                .ok()
        } else {
            None
        };

        let mut languages = Vec::new();
        let mut package_managers = Vec::new();
        let mut build_scripts = Vec::new();
        let mut test_scripts = Vec::new();
        let mut lint_scripts = Vec::new();

        if repo_path.join("package.json").exists() {
            languages.push("TypeScript/JavaScript".to_string());
            package_managers.push("npm/pnpm/yarn".to_string());
            test_scripts.push("npm test".to_string());
            lint_scripts.push("npm run lint".to_string());
            build_scripts.push("npm run build".to_string());
        }

        if repo_path.join("Cargo.toml").exists() {
            languages.push("Rust".to_string());
            package_managers.push("Cargo".to_string());
            test_scripts.push("cargo test".to_string());
            lint_scripts.push("cargo clippy".to_string());
            build_scripts.push("cargo build".to_string());
        }

        if repo_path.join("pyproject.toml").exists() || repo_path.join("requirements.txt").exists()
        {
            languages.push("Python".to_string());
            test_scripts.push("pytest".to_string());
        }

        if repo_path.join("go.mod").exists() {
            languages.push("Go".to_string());
            test_scripts.push("go test ./...".to_string());
        }

        let has_ci = repo_path.join(".github").join("workflows").exists()
            || repo_path.join(".gitlab-ci.yml").exists();
        let has_instruction_file = repo_path.join("SKILL.md").exists()
            || repo_path.join("AGENTS.md").exists()
            || repo_path.join("CLAUDE.md").exists();

        RepoInspectionResult {
            is_git_repo: is_git,
            active_branch,
            remote_url,
            languages,
            package_managers,
            build_scripts,
            test_scripts,
            lint_scripts,
            has_ci,
            has_instruction_file,
        }
    }
}

fn is_symlink_or_reparse_point(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_remove_dir_all_normal_nested_directory() {
        let temp = std::env::temp_dir().join(format!("axf_test_nested_{}", uuid::Uuid::new_v4()));
        let sub = temp.join("a").join("b").join("c");
        std::fs::create_dir_all(&sub).unwrap();
        let file = sub.join("data.txt");
        std::fs::write(&file, "hello").unwrap();

        // Make file readonly to test permission clearing
        let mut perms = std::fs::metadata(&file).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&file, perms).unwrap();

        assert!(temp.exists());
        GitService::safe_remove_dir_all(&temp).unwrap();
        assert!(!temp.exists());
    }

    #[test]
    fn test_safe_remove_dir_all_does_not_follow_symlinks() {
        let base = std::env::temp_dir().join(format!("axf_test_symlink_{}", uuid::Uuid::new_v4()));
        let external_dir = base.join("external_target");
        let managed_dir = base.join("managed_worktree");

        std::fs::create_dir_all(&external_dir).unwrap();
        std::fs::create_dir_all(&managed_dir).unwrap();

        let external_file = external_dir.join("critical.txt");
        std::fs::write(&external_file, "DO NOT DELETE").unwrap();

        let link_path = managed_dir.join("link_to_external");

        #[cfg(target_os = "windows")]
        {
            // On Windows, try junction via mklink or symlink
            let status = std::process::Command::new("cmd")
                .args([
                    "/c",
                    "mklink",
                    "/J",
                    &link_path.to_string_lossy(),
                    &external_dir.to_string_lossy(),
                ])
                .output();
            if status.is_err() || !link_path.exists() {
                // If unprivileged/disabled, fallback to file link or skip
                let _ = std::os::windows::fs::symlink_dir(&external_dir, &link_path);
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::os::unix::fs::symlink(&external_dir, &link_path).unwrap();
        }

        if link_path.exists() || std::fs::symlink_metadata(&link_path).is_ok() {
            // Delete managed worktree
            GitService::safe_remove_dir_all(&managed_dir).unwrap();

            // Managed dir must be gone
            assert!(!managed_dir.exists());

            // External dir and its contents MUST be completely intact!
            assert!(external_dir.exists(), "External directory was destroyed!");
            assert_eq!(
                std::fs::read_to_string(&external_file).unwrap(),
                "DO NOT DELETE",
                "External file content was modified/deleted!"
            );
        }

        // Clean up external dir
        let _ = GitService::safe_remove_dir_all(&base);
    }

    #[test]
    fn test_is_managed_worktree_rejects_unmanaged_candidate() {
        let git = GitService::new();
        let repo_path = PathBuf::from("B:\\TestRepo");
        let managed_root = PathBuf::from("C:\\Users\\test\\.agentxflow\\worktrees");
        let hostile_candidate = PathBuf::from("C:\\Windows\\System32");

        let result = git.is_managed_worktree(&repo_path, &[managed_root], &hostile_candidate);
        assert!(
            !result,
            "Hostile path outside managed roots must be rejected"
        );
    }

    #[test]
    fn test_is_managed_worktree_rejects_unmanaged_task_directory() {
        let git = GitService::new();
        let repo_path = PathBuf::from("B:\\TestRepo");
        let managed_root = PathBuf::from("C:\\Users\\test\\.agentxflow\\worktrees");
        // Path has name "task-123" but is not inside managed_root
        let fake_task_dir = PathBuf::from("D:\\random\\folder\\task-123");
        let result = git.is_managed_worktree(&repo_path, &[managed_root], &fake_task_dir);
        assert!(!result, "Unmanaged directory named task-* must be rejected");
    }

    #[test]
    fn test_is_managed_worktree_accepts_nested_task_directory() {
        let git = GitService::new();
        let repo_path = PathBuf::from("B:\\TestRepo");
        let managed_root = PathBuf::from("C:\\Users\\test\\.agentxflow\\worktrees");
        let valid_nested_task = managed_root.join("proj_123").join("task-456");

        let result = git.is_managed_worktree(&repo_path, &[managed_root], &valid_nested_task);
        assert!(
            result,
            "Nested task directory under managed root must be accepted"
        );
    }

    #[test]
    fn test_is_managed_worktree_rejects_repo_path_itself() {
        let git = GitService::new();
        let repo_path = PathBuf::from("C:\\Users\\test\\.agentxflow\\worktrees\\repo");
        let managed_root = PathBuf::from("C:\\Users\\test\\.agentxflow\\worktrees");

        let result = git.is_managed_worktree(&repo_path, &[managed_root], &repo_path);
        assert!(
            !result,
            "Primary repository path must NEVER be identified as a deletable worktree"
        );
    }

    #[test]
    fn test_run_git_cmd_large_output_does_not_deadlock() {
        let git = GitService::new();
        let dir = std::env::temp_dir().join(format!("test_git_large_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let _ = git.init_repo(&dir);
        let _ = git.run_git_cmd(&dir, &["config", "user.name", "AgentXFlow Test"]);
        let _ = git.run_git_cmd(&dir, &["config", "user.email", "test@agentxflow.local"]);

        let large_file = dir.join("large.txt");
        // Write 300KB of content
        let content = "AgentXFlow robust pipe streaming test line \n".repeat(7000);
        std::fs::write(&large_file, &content).unwrap();

        git.run_git_cmd(&dir, &["add", "large.txt"]).unwrap();
        git.run_git_cmd(&dir, &["commit", "-m", "Large commit"])
            .unwrap();

        // Query diff/log with large output (would deadlock if stdout was not drained concurrently)
        let output = git.run_git_cmd(&dir, &["log", "-p", "-n", "1"]);
        assert!(
            output.is_ok(),
            "Subprocess with large output must succeed without deadlocking"
        );

        let _ = GitService::safe_remove_dir_all(&dir);
    }

    #[test]
    fn test_init_repo_establishes_required_local_identity_and_creates_commit() {
        let git = GitService::new();
        let dir = std::env::temp_dir().join(format!("test_git_init_id_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        // 1. Initialize repo
        let init_res = git.init_repo(&dir);
        assert!(
            init_res.is_ok(),
            "init_repo must succeed: {:?}",
            init_res.err()
        );

        // 2. Verify local git configuration exists
        let user_name = git.run_git_cmd(&dir, &["config", "--local", "user.name"]);
        assert_eq!(user_name.as_deref(), Ok("AgentXFlow"));

        let user_email = git.run_git_cmd(&dir, &["config", "--local", "user.email"]);
        assert_eq!(user_email.as_deref(), Ok("agentxflow@local"));

        // 3. Create and commit a new file with no global identity configured
        let test_file = dir.join("probe.txt");
        std::fs::write(&test_file, "probe verification content\n").unwrap();
        git.run_git_cmd(&dir, &["add", "probe.txt"]).unwrap();

        let commit_res = git.run_git_cmd(&dir, &["commit", "-m", "Probe commit verification"]);
        assert!(
            commit_res.is_ok(),
            "Commit using repository-local identity must succeed: {:?}",
            commit_res.err()
        );

        let head_log = git.run_git_cmd(&dir, &["log", "-n", "1", "--pretty=format:%an <%ae>"]);
        assert_eq!(
            head_log.as_deref(),
            Ok("AgentXFlow <agentxflow@local>"),
            "Author identity in commit must match local repository config"
        );

        let _ = GitService::safe_remove_dir_all(&dir);
    }
}
