# AgentXFlow

**Multi-Agent Software Engineering Coordinator & Local MCP Server**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v2-24C8D8.svg)](https://v2.tauri.app/)
[![React](https://img.shields.io/badge/React-19-61DAFB.svg)](https://react.dev/)
[![MCP](https://img.shields.io/badge/MCP-2024--11--05-8A2BE2.svg)](https://modelcontextprotocol.io/)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](#)

AgentXFlow is an open-source, local-first desktop application and Model Context Protocol (MCP) coordinator daemon. It allows multiple AI coding agents—such as Google Antigravity, Cursor, Claude Code, OpenAI Codex, Windsurf, and OpenCode—to work concurrently on the same codebase without merge conflicts, file clobbering, or broken builds.

Developed by [harlixay7](https://github.com/harlixay7) • AgentXFlow by Viducia

---

## Why Multi-Agent Coding Breaks

When multiple autonomous coding assistants work on the same repository in parallel, standard developer workflows face common points of failure:

- **Concurrent File Overwrites**: Multiple agents edit the same files at the same time, overwriting each other's progress.
- **Unverified Assertions**: Large language models often report that code compiles or tests pass without actually executing build commands or test runners.
- **Branch Divergence**: Uncoordinated branches diverge rapidly, resulting in complex merge conflicts that require manual resolution.
- **Orphaned State**: Agents can claim large task backlogs, encounter errors or timeouts, and leave uncommitted changes or abandoned branches behind.

---

## How AgentXFlow Coordinates Parallel Agents

AgentXFlow acts as a centralized local coordinator to enforce process isolation and automated quality gates:

1. **Isolated Git Worktrees**: Every task runs inside its own isolated Git worktree on disk (`.agentxflow/worktrees/task-xxx`). Agents never write directly to the primary checkout while working.
2. **Deterministic Write Leases**: Before an agent can modify files, it acquires an exclusive path lease (e.g. `src/auth/**`). Overlapping lease requests are rejected by the coordinator.
3. **Automated Machine Verification**: Rather than relying on agent self-reporting, the coordinator executes project test suites (`cargo test`, `npm test`, linter, compiler) directly inside the agent's worktree.
4. **Cryptographic Proof Bundles**: When verification passes, the coordinator generates an append-only, tamper-evident SHA-256 Proof Bundle sealing the task metadata, git diffs, and evaluator output.
5. **Serialized FIFO Merge Queue**: Tasks that pass verification enter a sequential merge queue. The coordinator integrates candidate branches in a dedicated worktree, re-runs tests against latest HEAD, and advances `main` via atomic compare-and-swap (CAS) git reference updates.
6. **Desktop Workbench**: A clean, high-density desktop interface built with Tauri v2 and React 19 provides real-time visibility into active tasks, write leases, the dependency graph, and the merge queue.

---

## Architecture Overview

```
                   +-------------------------------------------------------------------+
                   |                         AI Coding Agents                          |
                   |       Antigravity • Cursor • Claude Code • Codex • Windsurf       |
                   +-------------------------------------------------------------------+
                                                     |
                                                     | Model Context Protocol (MCP 2024-11-05)
                                                     v HTTP JSON-RPC 2.0 (127.0.0.1:7890/mcp)
+------------------------------------------------------------------------------------------------------+
|                                    AgentXFlow Coordinator Daemon                                     |
|                                                                                                      |
|  [ Masterplan Hub ]      [ Scope Engine ]         [ Policy Engine ]      [ Verification Engine ]     |
|  - Task Decomposition    - File Glob Leases       - Execution Rules      - Cargo / NPM Evaluators    |
|  - Step Reservations     - Mutation Auditing      - Guardrail Policies   - SHA-256 Proof Bundles     |
|                                                                                                      |
|  [ SQLite WAL Database (Migrations 1–18) ]     [ Serialized FIFO Integration Worktree Queue (CAS) ]  |
+------------------------------------------------------------------------------------------------------+
           |                                         |                                     |
           v                                         v                                     v
+-----------------------+                 +-----------------------+             +-----------------------+
|  Git Worktree #1      |                 |  Git Worktree #2      |             | Integration Worktree  |
|  Branch: task-auth-1  |                 |  Branch: task-ui-2    |             | .agentxflow/integr... |
|  Agent: Antigravity   |                 |  Agent: Cursor        |             | CAS -> refs/heads/main|
+-----------------------+                 +-----------------------+             +-----------------------+
```

---

## Quick Start

### Prerequisites
- **Node.js**: v20 or higher
- **Rust**: 1.80 or higher (`cargo`, `rustc`)
- **Git**: 2.38 or higher with worktree support

### Quick Launch (Windows)
```cmd
:: 1. Run setup script to install dependencies and build assets
setup.bat

:: 2. Launch AgentXFlow
run.bat
```

### Manual Setup (macOS / Linux / Windows)
```bash
# 1. Clone the repository
git clone https://github.com/harlixay7/AgentXFlow.git
cd AgentXFlow

# 2. Install dependencies and build frontend assets
npm install
npm run build

# 3. Launch Tauri application in development mode
npm run tauri dev
```

---

## Connecting AI Coding Agents via MCP

AgentXFlow hosts a local Model Context Protocol (MCP) server conforming to JSON-RPC 2.0 at `http://127.0.0.1:7890/mcp`.

Copy your active Bearer token from the **Integrations** tab in the desktop application.

### Supported IDE & Agent Configurations

#### 1. Cursor (`.cursor/mcp.json`)
```json
{
  "mcpServers": {
    "agentxflow": {
      "url": "http://127.0.0.1:7890/mcp",
      "transport": "http",
      "headers": {
        "Authorization": "Bearer <YOUR_COORDINATOR_TOKEN>"
      }
    }
  }
}
```

#### 2. Claude Code & Claude Desktop (`claude_desktop_config.json`)
```json
{
  "mcpServers": {
    "agentxflow": {
      "url": "http://127.0.0.1:7890/mcp",
      "headers": {
        "Authorization": "Bearer <YOUR_COORDINATOR_TOKEN>"
      }
    }
  }
}
```

#### 3. Google Antigravity
Antigravity integrates via the skill definition. Add [`SKILL.md`](SKILL.md) to your workspace or run Antigravity alongside AgentXFlow.

#### 4. OpenCode CLI (`.mcp.json`)
```json
{
  "mcpServers": {
    "agentxflow": {
      "url": "http://127.0.0.1:7890/mcp",
      "transport": "http",
      "headers": {
        "Authorization": "Bearer <YOUR_COORDINATOR_TOKEN>"
      }
    }
  }
}
```

#### 5. Codeium Windsurf & OpenAI Codex
Connect via HTTP JSON-RPC 2.0 endpoint `http://127.0.0.1:7890/mcp` using the Bearer Authorization header.

---

## Standard Agent Coordination Workflow

AgentXFlow organizes work through a standardized sequence of MCP tool calls:

```
[ 1. Discovery & Registration ]
  agentxflow_current_context -> Returns active project, assigned task, and next recommended action.
  agent_register             -> Registers canonical agent name ("Antigravity", "Cursor", "Claude Code").

[ 2. Planning & Chunking ]
  masterplan_get             -> Reads the active masterplan.
  masterplan_decompose       -> Normalizes raw specifications into structured steps.
  masterplan_claim_chunk     -> Atomically reserves a batch of steps and provisions an isolated Git worktree.

[ 3. Lease Acquisition & Implementation ]
  scope_acquire              -> Locks file patterns (e.g. ['src/core/**', 'tests/core_test.rs']).
  task_workspace_write/read  -> Reads and writes code inside the isolated Git worktree.

[ 4. Verification & Submission ]
  task_complete_step         -> Records command verification output for each completed step.
  task_submit                -> Triggers automated test evaluators, audits diffs, and creates ProofBundle.

[ 5. Serialized Integration ]
  merge_enqueue              -> Verified task enters the serialized FIFO merge queue.
  merge_process              -> Background worker tests merge in integration worktree and CAS updates main.
```

---

## Complete MCP Tool Reference

| Tool Name | Access Role | Required Parameters | Purpose & Description |
|---|---|---|---|
| `agentxflow_current_context` | Any | `agent_id?`, `project_id?` | Discover active task, allocated worktree path, held scope leases, and next action. |
| `project_list` | Any | _(none)_ | List all managed projects, repository root paths, and target branches. |
| `project_context` | Any | `project_id`, `task_id?` | Fetch project architectural contract, rules, and task specification pack. |
| `masterplan_list` | Any | _(none)_ | Catalog all project masterplans, status states, and active chunk reservations. |
| `masterplan_get` | Any | `project_id` | Read active specification text, step count target, and architect instructions. |
| `masterplan_status` | Any | `project_id` | Query aggregate progress statistics, completed steps, and blockers. |
| `masterplan_reset` | Master | `project_id`, `masterplan_id?` | Safe reset: cancels tasks, deletes plan records, and prunes worktrees safely. |
| `prepare_masterplan` | Master | `project_id`, `raw_text`, `target_step_count?` | Save, parse, and structure a masterplan for agent decomposition. |
| `masterplan_decompose` | Master | `project_id`, `steps`, `append?`, `idempotency_key?` | Store structured execution steps with SHA-256 idempotency checking. |
| `masterplan_claim_chunk` | Agent / Master | `project_id`, `agent_id`, `count?` | Atomically reserve steps, bind task, and provision isolated Git worktree on disk. |
| `agent_register` | Agent / Master | `name`, `agent_type` | Register canonical agent identity and obtain session token. |
| `agent_heartbeat` | Agent / Master | `agent_id`, `waiting_for_permission?` | Refresh activity timestamp; activates 30-minute grace window when waiting for user input. |
| `task_list` | Any | `project_id` | Query tasks in Backlog, Ready, Running, Review, and Done states. |
| `task_get` | Any | `task_id` | Get task prompt, acceptance criteria, and authoritative worktree path. |
| `task_details` | Any | `task_id` | Fetch full task telemetry including attempts, scope leases, and verification records. |
| `task_claim` | Agent / Master | `task_id`, `agent_id` | Claim a task and provision isolated worktree branch with active attempt record. |
| `scope_acquire` | Task Owner / Master | `task_id`, `agent_id`, `patterns` | Lock file globs for exclusive write access. Caller must own the task. |
| `scope_release` | Lease Owner / Master| `task_id`, `agent_id?` | Release held write locks back to the project pool. |
| `task_complete_step` | Task Owner / Master | `step_id`, `agent_id?`, `evidence?` | Mark required task step complete with test output evidence. |
| `dag_dependencies` | Any | `task_id` | Query prerequisite blocker tasks (`BLOCKS`, `PARENT_CHILD`, `RELATED_TO`). |
| `task_submit` | Task Owner / Master | `task_id`, `agent_id` | Run machine evaluators, audit diff mutations, generate ProofBundle, and enqueue. |
| `task_cancel` | Task Owner / Master | `task_id`, `agent_id?`, `reason?` | Cancel active task, release write locks, clean worktree, and revert steps to PENDING. |
| `task_requeue` | Task Owner / Master | `task_id`, `agent_id?` | Requeue task back to pending masterplan steps and clean up worktree. |
| `unclaim_agent_tasks` | Master | `agent_id` | Unclaim all tasks for an agent and revert steps back to PENDING. |
| `force_agent_idle` | Master | `agent_id` | Reset agent state to IDLE and clean up orphaned reservations. |
| `task_reconcile` | Master | `task_id` | Reconcile task attempts, scope leases, proof bundles, and merge queue state. |
| `merge_queue_status` | Any | `project_id` | Inspect queue depth and positions for serialized branch integration. |
| `merge_enqueue` | Task Owner / Master | `project_id`, `task_id` | Enqueue a verified task into the serialized merge queue. |
| `merge_process` | Master | `project_id` | Process next ready candidate merge via isolated integration worktree and CAS commit. |
| `task_workspace_path` | Task Owner / Master | `task_id`, `agent_id` | Resolve the isolated worktree directory for a claimed task. |
| `task_workspace_read` | Task Owner / Master | `task_id`, `agent_id`, `file_path` | Read a file inside the task's isolated worktree with directory containment checks. |
| `task_workspace_write` | Task Owner / Master | `task_id`, `agent_id`, `file_path`, `content`| Write a file in the task's worktree, verifying write scope leases and directory containment. |
| `task_workspace_exec` | Task Owner / Master | `task_id`, `agent_id`, `command`, `args?` | Execute a command inside the task's worktree with timeout and process tree killing. |

---

## Quality Verification & Test Suite

AgentXFlow includes automated test suites across the Rust backend, TypeScript frontend, and integration bridge:

```bash
# 1. Run full Rust backend test suite (171+ tests across unit, integration, and security suites)
cargo test --manifest-path src-tauri/Cargo.toml --all-targets

# 2. Run complete end-to-end pipeline test
cargo test --test pipeline_a_to_z_test --manifest-path src-tauri/Cargo.toml

# 3. Run adversarial concurrency and security test suite
cargo test --test adversarial_suite_test --manifest-path src-tauri/Cargo.toml

# 4. Strict Rust linter check (zero warnings allowed)
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# 5. Rust format check
cargo fmt --manifest-path src-tauri/Cargo.toml --check

# 6. TypeScript type-check
npm test

# 7. Production frontend Vite build
npm run build
```

---

## Repository Structure

```
AgentXFlow/
├── src-tauri/                   # Rust Core & Tauri Desktop Host
│   ├── src/
│   │   ├── core/                # Coordinator state machine, workspace proxies, IPC endpoints
│   │   ├── mcp/                 # Model Context Protocol (MCP 2024-11-05) Axum HTTP server
│   │   ├── scope/               # Glob pattern collision detection & mutation audit engine
│   │   ├── policies/            # Deterministic policy engine with longest-prefix matching
│   │   ├── security/            # Constant-time token comparison & cryptographic session storage
│   │   ├── dag/                 # Directed Acyclic Graph dependency scheduler
│   │   ├── verification/        # Automated test evaluators & SHA-256 ProofBundle sealing
│   │   ├── merge/               # Serialized FIFO merge queue & disposable integration worktree
│   │   ├── scheduler/           # Background reconciliation & stale agent reclamation sweeps
│   │   ├── db/                  # SQLite migrations (1–18), WAL mode, and mutex locking
│   │   └── git/                 # Git worktree lifecycle, porcelain parsing, and CAS ref advancement
│   └── tests/                   # Concurrency, security, and pipeline test suites
├── src/                         # React 19 Frontend Workbench
│   ├── components/              # Mission Control, Work View, Review Center, etc.
│   ├── api/                     # Tauri IPC client & coordinator API bindings
│   └── index.css                # High-contrast design tokens, 2px/6px radii, tabular numbers
├── scripts/                     # MCP client helper & standalone offline Node MCP bridge
├── docs/                        # Specifications, architecture blueprints, and audit records
├── AGENT_SETUP.md               # Quickstart guide for AI agents and automated workflows
├── SKILL.md                     # Canonical Google Antigravity & AI Agent Skill specification
└── package.json                 # Project manifest & build scripts
```

---

## License

This project is licensed under the [MIT License](LICENSE).
