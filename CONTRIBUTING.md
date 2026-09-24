# Contributing to AgentXFlow

Thank you for your interest in contributing to AgentXFlow! AgentXFlow is an authoritative local-first coordinator for autonomous AI engineering agents.

---

## 1. Development Environment Setup

### Prerequisites
- **Rust Toolchain**: 1.80+ (`rustup default stable`)
- **Node.js**: 20+ (`npm install -g npm@latest`)
- **Git**: 2.38+ with worktree support

### Quickstart
1. Clone the repository:
   ```bash
   git clone https://github.com/harlixay7/AgentXFlow.git
   cd AgentXFlow
   ```
2. Install frontend dependencies:
   ```bash
   npm install
   ```
3. Run the development environment:
   ```bash
   npm run tauri dev
   ```

---

## 2. Architecture Guidelines & Invariants

AgentXFlow adheres to strict architectural invariants:
- **Authoritative Control Plane**: All file operations, Git mutations, worktrees, and test executions happen through coordinator-supervised Rust backend services.
- **Role-Based Authorization & Task Ownership**: Administrative tools (`masterplan_reset`, `prepare_masterplan`, `masterplan_decompose`, `merge_process`, `unclaim_agent_tasks`, `force_agent_idle`, `task_reconcile`) require Master authority. Task mutations (`scope_acquire`, `scope_release`, `task_complete_step`, `task_submit`, `task_cancel`, `task_requeue`, `merge_enqueue`) enforce that the caller owns the task.
- **Zero Self-Certification**: Autonomous agents must NEVER self-satisfy criteria or mark their own work valid. Evidence is produced by the agent/worktree, but evaluation and state derivation are strictly performed by the coordinator.
- **Preflight Validation for Destructive Operations**: Destructive operations (such as `reset_masterplan`) must validate all affected tasks upfront before mutating any database record or deleting any worktree. If any candidate cannot be cancelled cleanly, the entire operation must abort without side effects.
- **Fail-Closed Error Handling**: Security checks, merge queue FIFO queries, concurrency locks, and scope audits must fail closed on error. Never swallow database or IO errors with `unwrap_or_default()` or `unwrap_or(0)` in safety-critical code paths.
- **Deterministic Policy Specificity**: Multi-pattern security hooks evaluate deterministically using most-specific matching (`longest pattern length wins`) with `id ASC` tie-breaking.
- **Attempt-Scoped Auditing**: Task attempts track mutations, evaluator results, and scope audits. Scope violations belong to specific attempts and clear when covered by acquired leases.
- **Model Context Protocol (MCP) Conformance**: Tools exposed over HTTP strictly adhere to the MCP JSON-RPC 2.0 specification (2024-11-05 standard) and are registered in `src-tauri/src/mcp/registry.rs`.
- **Dynamic Security**: Never hardcode authentication tokens or secrets. Use `SecurityManager` to generate cryptographic tokens stored in local data directories. All token comparisons must use constant-time operations.
- **Adversarial Resilience**: All state transitions, scope reservations, and merge operations must maintain 100% unit and hostile adversarial test coverage.

---

## 3. Running Tests & Quality Gates

Before submitting a pull request, ensure all test suites pass with zero warnings:
```bash
# Run all backend unit and integration test suites across the workspace (171+ tests)
cargo test --manifest-path src-tauri/Cargo.toml --all-targets

# Run hostile adversarial security & concurrency suite
cargo test --test adversarial_suite_test --manifest-path src-tauri/Cargo.toml

# Run full A-to-Z pipeline test
cargo test --test pipeline_a_to_z_test --manifest-path src-tauri/Cargo.toml

# Run code format check across all crates
cargo fmt --manifest-path src-tauri/Cargo.toml --check

# Run Rust linter across all targets (zero warnings allowed)
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# Run TypeScript type check
npm test

# Verify TypeScript and build production bundle
npm run build
```

---

## 4. Code Style & Documentation

- Maintain concise, precise systems-engineering terminology.
- All code comments and documentation must follow standard technical style guidelines.
- Commit messages follow the Conventional Commits specification.
