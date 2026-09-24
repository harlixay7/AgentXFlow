# AgentXFlow

**AgentXFlow** is an authoritative desktop application and Model Context Protocol (MCP) coordination daemon that enables multiple AI coding agents to collaborate concurrently on a single Git repository through isolated worktrees, write scope leases, automated machine verification gates, and a serialized FIFO merge queue.

Developed by **[harlixay7](https://github.com/harlixay7)** • **AgentXFlow by Viducia**

---

## Core Capabilities

- **Isolated Git Worktrees**: When an agent claims a task or masterplan chunk, AgentXFlow allocates an isolated Git worktree on disk. Agents work exclusively within their assigned worktree branch.
- **Write Scope Leases**: Agents declare intended file glob patterns (e.g. `src/auth/**`) before modifying files. Overlapping write patterns are detected and rejected.
- **Attempt-Scoped Mutation Auditing**: On task submission, the coordinator audits `git diff` against active scope leases for the current attempt. Acquiring missing scope leases cleanly resolves unreserved file violations on re-run.
- **Automated Machine Verification Gates**: The coordinator automatically executes verification profiles and machine evaluators (e.g. cargo test, npm test, type checks, lint checks) directly in the worktree. Criteria satisfaction is derived strictly from passing evaluator results (zero autonomous self-certification).
- **Cryptographic Proof Bundles**: Verified submissions generate an append-only, tamper-evident `ProofBundle` sealed with a SHA-256 digest over task metadata, file diffs, and test outputs (hash re-verified at the merge gate).
- **Serialized FIFO Merge Queue & Fail-Closed Safety**: Verified candidate branches are integrated sequentially inside disposable integration worktrees, validated with post-merge tests, and advanced via atomic Compare-and-Swap (CAS) `git update-ref` operations. Queue state checks strictly fail closed on read failures.
- **Preflight-Safe Masterplan Reset & Blast Radius Isolation**: Reset operations preflight all candidate tasks before mutating state. Explicit masterplan reset (`masterplan_id`) cancels only target plan tasks and prunes git worktrees without touching other masterplans or their worktrees. The primary checkout is never modified.
- **Role-Based MCP Authorization & Task Ownership**: Strict authority separation: administrative tools require Master authority, and task mutation tools require the caller to be the assigned owner of the task. Cross-agent impersonation is blocked.
- **Deterministic Policy Engine**: Overlapping security policies resolve deterministically using most-specific pattern precedence (`longest pattern length wins`) with `id ASC` tie-breaking. Default actions fail closed.
- **Multi-Masterplan Catalog & Single-Active Toggle**: Manage multiple architecture specifications per project with an active/inactive toggle switch. Strictly enforces single-active mutual exclusion with conflict resolution modals, ensuring AI agents only see, decompose, and claim from the currently published masterplan.
- **Dynamic Agent State Engine & Transparent Activity Heartbeats**: Real-time evaluation of agent liveness (`WORKING`, `IDLE`, `DISCONNECTED`) derived from active task assignments and a 120-second activity window. Every inbound MCP tool call automatically refreshes agent timestamps—zero background timer loops required from LLM agents.
- **Cryptographic Session Lifetimes**: Session tokens are cryptographically unpredictable with a 30-day sliding activity window and a 365-day maximum lifetime.
- **Atomic Project Creation**: Project initialization, contract generation, and baseline rule seeding execute within a single atomic database transaction with deterministic rollback on child failure.
- **Interactive Step Unclaiming & Worktree Cleanup**: 1-click step recovery in both desktop UI and MCP gateway. Easily unclaim orphaned in-flight tasks from disconnected agents, reverting steps back to `PENDING`, releasing scope locks, and wiping isolated worktrees safely.
- **Expanded Capacity Limits**: Scalable masterplan decomposition supporting up to **100 structured steps** and anti-hoarding chunk caps up to **8 steps per agent**.
- **Masterplan Hub**: Single atomic preparation operation (`prepare_masterplan`) saves revisions, parses specification text, structures steps, and normalizes scopes with anti-hoarding active claim limits.

---

## Architecture

```
+---------------------------------------------------------------+
|                          AI Agents                            |
|      Antigravity       Claude Code       Cursor       Codex   |
+---------------------------------------------------------------+
                                |
                                | (HTTP JSON-RPC / MCP 2024-11-05)
                                v
+---------------------------------------------------------------+
|                     AgentXFlow Coordinator                    |
|   - Masterplan Hub (Decomposition & anti-hoarding chunking)   |
|   - SQLite Database (Versioned migrations 1–16, WAL mode)    |
|   - Scope Engine (Multi-glob splitting & attempt auditing)    |
|   - Policy Engine (Deterministic most-specific matching)     |
|   - Security Engine (Role authority & session lifetimes)     |
|   - Verification Engine (Machine evaluators & profile runner) |
|   - Merge Queue Engine (Serialized FIFO 3-way integration)    |
|   - DAG Scheduler (Dependency-type-aware execution gating)    |
+---------------------------------------------------------------+
         |                      |                      |
         v                      v                      v
    Worktree #1            Worktree #2            Integration Worktree
 (task branch A)        (task branch B)           (merges to target)
```

---

## Getting Started

### Prerequisites
- Node.js 20 or higher
- Rust 1.80 or higher (`cargo` and `rustc`)
- Git CLI

### 1-Click Setup (Windows)
Double-click **`setup.bat`** (or run `.\setup.bat` in terminal). This script:
1. Verifies Node.js, Git CLI, and the Rust toolchain.
2. Installs required npm dependencies.
3. Compiles and type-checks the React frontend.
4. Validates Rust backend compilation.

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

---

## Connecting AI Agents via MCP

AgentXFlow hosts a local Model Context Protocol (MCP) server conforming to the `2024-11-05` standard on `http://127.0.0.1:7890/mcp`.

Authentication tokens are generated dynamically per coordinator instance. Copy your active token from the **MCP Gateway** tab in the desktop application.

### Supported Canonical AI IDEs & Tools

| IDE / Client | Agent Name in `agent_register` | Connection Type |
|---|---|---|
| **Google Antigravity** | `Antigravity` | Native MCP / Skill |
| **Claude Code** | `Claude Code` | MCP Gateway / CLI |
| **Cursor AI** | `Cursor` | MCP `.cursor/mcp.json` |
| **OpenCode** | `OpenCode` | MCP Gateway / IDE |
| **OpenAI Codex** | `OpenAI Codex` | MCP Gateway / CLI |
| **Google Gemini CLI** | `Gemini CLI` | MCP Gateway / CLI |
| **GitHub Copilot** | `GitHub Copilot` | MCP / VS Code Bridge |
| **Codeium Windsurf** | `Windsurf` | MCP Cascade Gateway |
| **JetBrains Junie** | `Junie` | MCP Integration |
| **Aider** | `Aider` | MCP / CLI Pair |

### Cursor (`.cursor/mcp.json`)
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

### Claude Desktop (`claude_desktop_config.json`) / Claude Code
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

### Antigravity
The canonical coordinator skill definition is located at [`SKILL.md`](SKILL.md).

### Standard Agent Startup Workflow
```
1. Context        -> Call agentxflow_current_context to discover active project, assigned task, and next action.
2. Register       -> Call agent_register(name="<Your_IDE>") with your canonical IDE platform (e.g. "Antigravity", "Claude Code", "Cursor").
3. Contract       -> Call project_context with project_id to fetch architectural rules and conventions.
4. Masterplan     -> Call masterplan_get. If UNSORTED, act as Master Architect: decompose raw specification into high-fidelity steps via masterplan_decompose.
5. Claim Chunk    -> Call masterplan_claim_chunk to allocate an isolated Git worktree (strictly capped by max_steps_per_agent).
6. Scope          -> Call scope_acquire with specific file globs before modifying code.
7. Implement      -> Make changes strictly inside your allocated worktree path and verify locally.
8. Step Evidence  -> Call task_complete_step with step_id and command verification evidence.
9. Submit & Gate  -> Call task_submit. The coordinator automatically executes verification profiles, audits scope mutations, generates ProofBundle, and enqueues to merge queue.
10. Milestone Handoff -> Inspect `next_action` in task_submit response:
                     - If `REPORT_TO_USER`: Interactive Milestone mode is active. Stop calling tools immediately, present a milestone walkthrough in chat, and wait for user confirmation.
                     - If `masterplan_claim_chunk`: Continuous Autonomous Swarm mode is active. Proceed immediately to claim the next chunk.
                     - If `FINAL_RELEASE_DELIVERY`: All masterplan steps are completed! As the final agent submitting Step N/N, build the production executable, generate the automated launcher (`run.bat`/`start.sh`), test that the app starts cleanly, create `USER_GUIDE.md` explaining the entire app, and present the full walkthrough to the user.
```

---

## MCP Tools Reference

| Tool | Authority | Parameters | Description |
|---|---|---|---|
| `agentxflow_current_context` | Any | `agent_id?`, `project_id?` | Get tailored context, active task, assigned worktree, active scopes, and recommended next action. |
| `project_list` | Any | _(none)_ | List all managed projects with exact IDs, repository paths, and target branches. |
| `project_context` | Any | `project_id`, `task_id?` | Fetch contract hash and project architectural rules (or full task context pack when `task_id` is supplied). |
| `masterplan_list` | Any | _(none)_ | List all masterplans across all projects with status, step counts, and active handoffs. |
| `masterplan_get` | Any | `project_id` | Inspect masterplan state, raw specification text, project identity, and architect decomposition instructions. |
| `masterplan_status` | Any | `project_id` | Query plan progress stats, total steps, and step statuses. |
| `masterplan_reset` | Master | `project_id`, `masterplan_id?` | Preflight-safe reset: verifies all candidate tasks are cancellable before mutating any state. When `masterplan_id` is specified, cancels only that plan's tasks, transactionally deletes plan records, and prunes git worktrees without touching unrelated plan worktrees. Project-wide reset cleans all plans and wipes `.agentxflow/worktrees`. User primary checkout is never modified. |
| `prepare_masterplan` | Master | `project_id`, `raw_text`, `target_step_count?`, `max_steps_per_agent?` | Atomically save, parse, structure, and prepare a masterplan for agents. |
| `masterplan_decompose` | Master | `project_id`, `steps`, `append?`, `idempotency_key?`, `compact?` | Normalize raw masterplan text into structured execution steps. Supports chunked decomposition with append mode, SHA-256 request hash idempotency checking, crash-atomic storage, and compact range continuation responses (`persisted_range`, `total_known`, `next_expected_range`, `continuation_required`). |
| `masterplan_claim_chunk`| Agent (Self) / Master | `project_id`, `agent_id`, `count?` | Atomically claim next batch of steps (capped by limit) and allocate an isolated Git worktree. Enforces single-transaction atomic step-task binding. |
| `agent_register` | Agent (Self) / Master | `name`, `agent_type` | Idempotently register agent session with a canonical IDE identity and get an authoritative session token. Master can register any; Agents can only self-refresh. |
| `agent_heartbeat` | Agent (Self) / Master | `agent_id`, `waiting_for_permission?`, `task_id?` | Refresh session heartbeat and active lease timers. When `waiting_for_permission: true` is sent, activates bounded 30-minute grace protection against stale reclamation while waiting for user/IDE input. |
| `task_list` | Any | `project_id` | List tasks in the backlog or ready queue for a specific project. |
| `task_get` | Any | `task_id` | Get task prompt, acceptance criteria, and worktree path. |
| `task_details` | Any | `task_id` | Get complete task details including steps, acceptance criteria, active scope leases, attempts, and verification results. |
| `task_claim` | Agent (Self) / Master | `task_id`, `agent_id` | Claim a task and create an isolated Git worktree on disk with an active task attempt. |
| `scope_acquire` | Task Owner / Master | `task_id`, `agent_id`, `patterns` | Lock file globs (e.g. `['src/auth/**', 'tests/auth_test.rs']`) for exclusive writes. Caller must own the task. |
| `scope_release` | Lease Owner / Master | `task_id`, `agent_id?` | Release held write locks back to the pool. |
| `task_complete_step` | Task Owner / Master | `step_id`, `agent_id?`, `evidence?` | Mark a required task step complete with test output. Caller must own the parent task. |
| `dag_dependencies` | Any | `task_id` | List blocker tasks that must finish before this task starts. Type-aware scheduling (`BLOCKS`, `PARENT_CHILD`, `RELATED_TO`). |
| `task_submit` | Task Owner / Master | `task_id`, `agent_id` | Submit task; coordinator automatically executes verification profiles, machine evaluators, and git diff mutation audit. Returns milestone handoff instructions on completion. |
| `task_cancel` | Task Owner / Master | `task_id`, `agent_id?`, `reason?` | Cancel an active task, releasing write scope leases, cleaning up worktrees, and reverting masterplan steps back to PENDING. |
| `task_requeue` | Task Owner / Master | `task_id`, `agent_id?` | Requeue a claimed chunk task back to masterplan pending steps, releasing held scope leases. |
| `unclaim_agent_tasks` | Master | `agent_id` | Safely unclaim all active tasks for an agent, reverting masterplan steps to PENDING and releasing locks. |
| `force_agent_idle` | Master | `agent_id` | Forces an agent status to IDLE, unclaiming active tasks and resetting session heartbeat. |
| `task_reconcile` | Master | `task_id` | Reconcile task state, task attempt, proof bundle, and merge queue status. |
| `merge_queue_status` | Any | `project_id` | Check queue position and status for serialized branch merges. |
| `merge_enqueue` | Task Owner / Master | `project_id`, `task_id` | Enqueue a verified or MERGE_READY task into the serialized merge queue. |
| `merge_process` | Master | `project_id` | Process next ready serialized branch merge in queue. Enforces strict FIFO serialization and fail-closed concurrency checks. |
| `task_workspace_path` | Task Owner / Master | `task_id`, `agent_id` | Authoritatively resolve the active worktree path for an assigned task. |
| `task_workspace_read` | Task Owner / Master | `task_id`, `agent_id`, `file_path` | Read a file from the task's authoritative isolated worktree without path confusion or directory traversal. |
| `task_workspace_write` | Task Owner / Master | `task_id`, `agent_id`, `file_path`, `content` | Write a file into the task's authoritative isolated worktree, verifying write scope leases and directory containment. |
| `task_workspace_exec` | Task Owner / Master | `task_id`, `agent_id`, `command`, `args?`, `timeout_seconds?` | Execute a command strictly inside the task's isolated worktree with timeout protection and process tree killing. |

---

## Testing & Quality Gates

```bash
# Run all backend unit and integration test suites across the workspace (171+ tests)
cargo test --manifest-path src-tauri/Cargo.toml --all-targets

# Run the complete A-to-Z pipeline integration test
cargo test --test pipeline_a_to_z_test --manifest-path src-tauri/Cargo.toml

# Run the 30-scenario adversarial security and concurrency test suite
cargo test --test adversarial_suite_test --manifest-path src-tauri/Cargo.toml

# Run Rust linter with zero warnings allowed
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# Format check across all crates
cargo fmt --manifest-path src-tauri/Cargo.toml --check

# Type-check TypeScript codebase
npm test

# Build production frontend bundle
npm run build
```

---

## Repository Structure

- `src-tauri/src/core/`: Coordinator engine, state machines, canonical IDE resolution, preflight reset safety, task workspace proxy execution, and Tauri IPC commands.
- `src-tauri/src/mcp/`: Model Context Protocol (MCP 2024-11-05) Axum HTTP server with role authorization and tool registry.
- `src-tauri/src/scope/`: Glob pattern collision detection, pattern normalization, and attempt-scoped mutation auditor.
- `src-tauri/src/policies/`: Deterministic policy engine with most-specific pattern precedence and fail-closed actions.
- `src-tauri/src/security/`: Session token management, constant-time token comparison, and cryptographic generation.
- `src-tauri/src/dag/`: Task dependency resolution, acyclic DAG validation, and type-aware scheduling (`BLOCKS`, `PARENT_CHILD`, `RELATED_TO`).
- `src-tauri/src/verification/`: Verification profiles, automated machine evaluators, and SHA-256 proof bundle generator.
- `src-tauri/src/merge/`: Serialized FIFO merge engine with disposable integration worktrees, fail-closed concurrency checks, pre-finalization target branch authority validation, and CAS ref updates.
- `src-tauri/src/scheduler/`: Background reconciliation, stale agent sweep, and merge queue worker.
- `src-tauri/src/db/`: Versioned SQLite migrations (1–17), serialized SQLite access (single connection, mutex-guarded), and single-instance file lock.
- `src/`: React 19 / TypeScript workbench UI.

---

## License

MIT ([`LICENSE`](LICENSE))
