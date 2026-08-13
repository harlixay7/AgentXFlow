# Viducia

Viducia is a local desktop application and coordination daemon that lets multiple AI coding agents work on the same Git repository simultaneously without merge conflicts, file overwrites, or broken builds.

Developer: **harlixay7**

---

## Quick Setup with an AI Agent

You can give this repository URL directly to your AI coding assistant (Claude Code, Cursor, Antigravity, or Codex CLI) and say:

```
Clone https://github.com/harlixay7/Viducia.git, read AGENT_SETUP.md, run setup.bat, and start the app with run.bat.
```

The AI agent will read [`AGENT_SETUP.md`](AGENT_SETUP.md), verify the toolchain, install dependencies, validate the test suites, and boot the coordinator.

---

## The Problem

When multiple AI agents (like Claude Code, Cursor, Antigravity, or Codex CLI) work on the same codebase at the same time:
1. They write to the same working directory and overwrite each other's changes.
2. They generate git branch conflicts that require manual resolution.
3. They claim tasks are completed in chat without actually running builds or test suites.
4. Concurrent merges into `main` break the build because changes were tested against outdated base commits.

---

## How Viducia Solves This

Viducia acts as an authoritative coordinator running on your machine:

- **Isolated Git Worktrees**: When an agent claims a task, Viducia creates a private Git worktree on disk at `.agentxflow/worktrees/task-<id>`. Agents never touch your active checkout or the `main` branch directly.
- **Write Scope Locks**: Agents must declare which files they plan to edit using glob patterns (e.g. `src/auth/**`). If two agents request overlapping files, the collision is detected and blocked.
- **Mutation Auditing**: On task submission, Viducia runs `git diff --name-only <base_sha>` inside the worktree. Any unreserved file edits trigger a scope violation and reject the task.
- **Authoritative Verification**: Viducia runs the test suite itself inside the worktree. An agent cannot mark a task done by claiming it in chat; it must pass the coordinator's automated checks.
- **Serialized Merge Queue**: Verified tasks enter a FIFO merge queue. Viducia simulates a 3-way merge and runs tests inside a hidden integration worktree before committing to `main`.
- **Masterplan Execution Hub**: Drop in an unformatted master plan of any length. The first connected agent normalizes the specification into structured steps (from `UNSORTED` to `RESORTED`). Subsequent agents claim progressive chunks (with anti-hoarding limits) and execute in sequence.

---

## Architecture

```
+---------------------------------------------------------------+
|                          AI Agents                            |
|   Antigravity     Claude Code CLI     Cursor      Codex CLI   |
+---------------------------------------------------------------+
                                |
                                | (HTTP JSON-RPC / MCP)
                                v
+---------------------------------------------------------------+
|                      Viducia Coordinator                      |
|   - Masterplan Hub (Unsorted -> Resorted decomposition engine)|
|   - SQLite Database (Tasks, Agents, Leases, Proof Bundles)    |
|   - Scope Engine (Glob lease manager & git diff auditor)      |
|   - Verification Engine (Independent test runner)             |
|   - Merge Engine (Serialized FIFO 3-way integration)          |
+---------------------------------------------------------------+
         |                      |                      |
         v                      v                      v
    Worktree #1            Worktree #2            Integration Worktree
 (task-auth branch)     (task-db branch)          (merges to main)
```

---

## Getting Started

### Prerequisites
- Node.js 20 or higher
- Rust 1.80 or higher (`cargo` and `rustc`)
- Git CLI

### 1-Click Setup (Windows)
Double-click **`setup.bat`** (or run `.\setup.bat` in terminal). This automated script:
1. Checks for Node.js, Git CLI, and Rust compiler.
2. Installs missing npm dependencies.
3. Builds and type-checks the React frontend.
4. Checks Rust backend compilation.

### Run the App
- On Windows: Double-click **`run.bat`**
- Or via terminal:
```bash
npm run tauri dev
```

### Build Production Binary
```bash
npm run tauri build
```

### Freeing Disk Space
During development, the Rust compiler stores debug symbols in `src-tauri/target/`. You can safely clean these temporary build files anytime without losing source code or database records:
```bash
cd src-tauri
cargo clean
```

---

## Running Tests

Viducia includes full unit and integration test suites:

```bash
# Run all backend unit tests
cargo test --manifest-path src-tauri/Cargo.toml

# Run the masterplan decomposition and chunk claiming test
cargo test --test masterplan_test --manifest-path src-tauri/Cargo.toml

# Run the live HTTP MCP network test
cargo test --test mcp_e2e_test --manifest-path src-tauri/Cargo.toml

# Run the 3-agent concurrent collaboration test
cargo test --test multi_agent_concurrent_test --manifest-path src-tauri/Cargo.toml

# Type check frontend
npm run build
```

---

## Connecting Coding Agents

Viducia runs a local Model Context Protocol (MCP) server on `http://127.0.0.1:7890/mcp`.

### OpenCode (`.mcp.json`)
Add this to your project repository:
```json
{
  "mcpServers": {
    "viducia": {
      "url": "http://127.0.0.1:7890/mcp",
      "transport": "http",
      "headers": {
        "Authorization": "Bearer axf_sec_v2_live_token_7890"
      }
    }
  }
}
```

### Claude Code & Codex CLI
Connect via HTTP streamable transport:
- Server URL: `http://127.0.0.1:7890/mcp`
- Header: `Authorization: Bearer axf_sec_v2_live_token_7890`

### Antigravity
Antigravity automatically loads the skill from `SKILL.md` or `.agents/skills/agentxflow-coordinator/SKILL.md`.

---

## MCP Tool Reference

| Tool | Parameters | Description |
|---|---|---|
| `agent.register` | `name`, `agent_type` | Register an agent session and declare its tool capabilities. |
| `agent.heartbeat` | `agent_id` | Keep session lease and file locks active. |
| `task.list` | `project_id`, `state` | List tasks in the backlog or ready queue. |
| `task.get` | `task_id` | Get task prompt, acceptance criteria, and worktree path. |
| `task.claim` | `task_id`, `agent_id` | Claim a task and create an isolated Git worktree on disk. |
| `project.context` | `project_id`, `task_id` | Fetch contract hash and project architectural rules. |
| `scope.acquire` | `task_id`, `patterns` | Lock file globs (e.g. `src/auth/**`) for exclusive writes. |
| `scope.release` | `task_id` | Release held write locks back to the pool. |
| `task.complete_step` | `step_id`, `evidence` | Mark a required task step complete with test output. |
| `dag.dependencies` | `task_id` | List blocker tasks that must finish before this task starts. |
| `task.submit` | `task_id`, `agent_id` | Submit task; coordinator runs tests and checks git diff. |
| `merge.queue_status` | `project_id` | Check queue position for pending branch merges. |

---

## Codebase Layout

- `src-tauri/src/core/`: Coordinator engine, state machines, and Tauri IPC commands.
- `src-tauri/src/mcp/`: Axum HTTP MCP server implementation (2026-07-28 protocol).
- `src-tauri/src/scope/`: Glob pattern collision detection and git diff mutation auditor.
- `src-tauri/src/verification/`: Server-side test runner and SHA-256 proof bundle generator.
- `src-tauri/src/merge/`: Serialized 3-way merge engine inside isolated integration worktree.
- `src-tauri/src/db/`: SQLite database pool, 24 tables with strict foreign keys, and WAL mode.
- `src/`: React 19 workbench user interface.

---

## Author

Created and developed by **[harlixay7](https://github.com/harlixay7)**.

## License

Apache-2.0
