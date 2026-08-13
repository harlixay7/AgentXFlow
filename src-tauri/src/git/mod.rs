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

#[derive(Debug, Clone)]
pub struct GitService;

impl GitService {
    pub fn new() -> Self {
        Self
    }

    pub fn run_git_cmd(&self, cwd: &Path, args: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("Failed to execute git command {:?}: {}", args, e))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            let err_msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(format!("Git error (code {:?}): {}", output.status.code(), err_msg))
        }
    }

    pub fn is_git_repo(&self, repo_path: &Path) -> bool {
        self.run_git_cmd(repo_path, &["rev-parse", "--is-inside-work-tree"])
            .map(|res| res == "true")
            .unwrap_or(false)
    }

    pub fn init_repo(&self, repo_path: &Path) -> Result<(), String> {
        std::fs::create_dir_all(repo_path).map_err(|e| format!("Failed to create folder {:?}: {}", repo_path, e))?;
        self.run_git_cmd(repo_path, &["init"])?;
        self.run_git_cmd(repo_path, &["checkout", "-b", "main"]).ok();
        if self.run_git_cmd(repo_path, &["rev-parse", "HEAD"]).is_err() {
            self.run_git_cmd(repo_path, &["commit", "--allow-empty", "-m", "Initial commit"]).ok();
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

    pub fn is_base_stale(&self, repo_path: &Path, base_sha: &str, target_branch: &str) -> bool {
        match self.get_ref_sha(repo_path, target_branch) {
            Ok(current_target_sha) => current_target_sha != base_sha,
            Err(_) => false,
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
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create worktree parent directory: {}", e))?;
        }

        info!("Creating git worktree at {:?} for branch {}", worktree_dir, branch_name);

        let branch_exists = self.run_git_cmd(repo_path, &["rev-parse", "--verify", branch_name]).is_ok();

        let mut args = vec!["worktree", "add"];
        let worktree_str = worktree_dir.to_str().ok_or("Invalid UTF-8 in worktree path")?;

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
            let res = self.run_git_cmd(repo_path, &["worktree", "add", "--detach", integration_str, target_branch]);
            if let Err(e) = res {
                warn!("Integration worktree add notice: {}", e);
            }
        }

        Ok(integration_dir)
    }

    pub fn remove_worktree(&self, repo_path: &Path, worktree_dir: &Path) -> Result<(), String> {
        let worktree_str = worktree_dir.to_str().ok_or("Invalid UTF-8 in worktree path")?;
        info!("Removing git worktree at {:?}", worktree_dir);

        if let Err(e) = self.run_git_cmd(repo_path, &["worktree", "remove", "--force", worktree_str]) {
            error!("Git worktree remove returned error: {}. Cleaning directory directly.", e);
        }

        self.run_git_cmd(repo_path, &["worktree", "prune"]).ok();

        if worktree_dir.exists() {
            std::fs::remove_dir_all(worktree_dir).map_err(|e| format!("Failed to delete worktree directory: {}", e))?;
        }

        Ok(())
    }

    pub fn get_diff(&self, repo_path: &Path, base_ref: &str, target_ref: &str) -> Result<String, String> {
        self.run_git_cmd(repo_path, &["diff", &format!("{}...{}", base_ref, target_ref)])
    }

    pub fn get_changed_files(&self, repo_path: &Path, base_ref: &str, target_ref: &str) -> Result<Vec<String>, String> {
        let output = self.run_git_cmd(repo_path, &["diff", "--name-only", &format!("{}...{}", base_ref, target_ref)])?;
        let files = output
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(files)
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
        let active_branch = if is_git { self.get_current_branch(repo_path).ok() } else { None };
        let remote_url = if is_git { self.run_git_cmd(repo_path, &["remote", "get-url", "origin"]).ok() } else { None };

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

        if repo_path.join("pyproject.toml").exists() || repo_path.join("requirements.txt").exists() {
            languages.push("Python".to_string());
            test_scripts.push("pytest".to_string());
        }

        if repo_path.join("go.mod").exists() {
            languages.push("Go".to_string());
            test_scripts.push("go test ./...".to_string());
        }

        let has_ci = repo_path.join(".github").join("workflows").exists() || repo_path.join(".gitlab-ci.yml").exists();
        let has_instruction_file = repo_path.join("SKILL.md").exists() || repo_path.join("AGENTS.md").exists() || repo_path.join("CLAUDE.md").exists();

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
