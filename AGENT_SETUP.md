# AI Agent Setup Guide — AgentXFlow

This document contains step-by-step instructions for AI agents (Claude Code, Antigravity, Cursor, Codex, OpenCode) tasked with setting up, building, and interacting with **AgentXFlow** (by **Viducia**).

Developer: **[harlixay7](https://github.com/harlixay7)**

---

## 1. Check Host Prerequisites

Verify host development tools before running builds:

```bash
node -v      # Node.js v20 or higher
npm -v
cargo -v     # Rust 1.80 or higher
git --version
```

If any prerequisite tool is missing, stop and inform the user.

---

## 2. Clone & Install Dependencies

```bash
# Clone the repository
git clone https://github.com/harlixay7/AgentXFlow.git
cd AgentXFlow

# On Windows: run automated setup script
setup.bat

# Or execute manual setup steps:
# 1. Install frontend dependencies
npm install

# 2. Check TypeScript types and build frontend assets
npm run build

# 3. Check Rust backend compilation
cargo check --manifest-path src-tauri/Cargo.toml
```

---

## 3. Run Quality Verification Gates

Verify that all test suites compile and pass 100%:

```bash
# 1. Run all backend unit and integration test suites across the workspace (151+ tests)
cargo test --manifest-path src-tauri/Cargo.toml --all-targets

# 2. Run complete A-to-Z pipeline integration test
cargo test --test pipeline_a_to_z_test --manifest-path src-tauri/Cargo.toml

# 3. Run 30-scenario hostile adversarial security test suite
cargo test --test adversarial_suite_test --manifest-path src-tauri/Cargo.toml

# 4. Verify code formatting across all crates
cargo fmt --manifest-path src-tauri/Cargo.toml --check

# 5. Verify linting (zero warnings enforced)
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# 6. Type-check TypeScript codebase
npm test

# 7. Build production frontend bundle
npm run build
```

---

## 4. Starting the Coordinator Application

To start the desktop application and boot the local Model Context Protocol (MCP) coordination server on `127.0.0.1:7890`:

- On Windows: double-click `run.bat`
- Or via terminal:
```bash
npm run tauri dev
```

---

## 5. Connecting AI Agents via MCP

AgentXFlow hosts an MCP server conforming to standard JSON-RPC 2.0 at `http://127.0.0.1:7890/mcp`.

Authentication tokens are generated dynamically per coordinator instance. Copy your active token from the **MCP Gateway** tab in the desktop application.

### OpenCode (`.mcp.json`)
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

### Cursor (`.cursor/mcp.json`)
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
The canonical coordinator skill definition is located at [`SKILL.md`](SKILL.md) and installed in `~/.gemini/config/skills/agentxflow-coordinator/SKILL.md`.

## 6. Supported Canonical AI IDE Platforms

| IDE / Client | Canonical Name | Connection Type |
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

---

## 7. Standard Agent Workflow & Principles

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
```

### Fundamental Coordination Invariants

- **Zero Self-Certification**: Autonomous agents cannot mark their own criteria valid or bypass verification. All criteria satisfaction is derived exclusively from passing automated machine evaluators executed by the coordinator.
- **Isolated Worktrees**: All code edits must occur inside `data_dir/AgentXFlow/worktrees/<project>/task-<id>` (primary) or the assigned AppData worktree path. Never edit the primary repository root directly.
- **Attempt-Scoped Auditing**: Scope violations are bound to your active `attempt_id`. Acquiring missing scope leases cleanly clears violations on subsequent re-runs and submissions.
- **Role-Based Authorization & Task Ownership**: Master authority is required for administrative coordinator actions (`merge_process`, `masterplan_reset`, `prepare_masterplan`, `masterplan_decompose`, `unclaim_agent_tasks`, `force_agent_idle`, `task_reconcile`). Task mutation tools (`scope_acquire`, `scope_release`, `task_complete_step`, `task_submit`, `task_cancel`, `task_requeue`, `merge_enqueue`) enforce strict task ownership and prevent cross-agent impersonation.
- **Preflight-Safe Masterplan Reset & Blast Radius Isolation**: Reset operations preflight all candidate tasks before mutating state. Explicit masterplan reset (`masterplan_id`) cancels only target plan tasks and prunes git worktrees without touching other masterplans or their worktrees. The primary checkout is never modified.
- **Deterministic Policy Precedence**: Security policies resolve deterministically using most-specific pattern precedence (`longest pattern length wins`) with `id ASC` tie-breaking. Default actions fail closed.
- **Fail-Closed Merge Queue Serialization**: Merge candidate queue checks strictly enforce FIFO order and prevent concurrent integration on the same target branch. If queue state cannot be authoritatively queried, operations fail closed with errors rather than defaulting to zero.
- **Hybrid Milestone Handoff**: When `require_milestone_approval` is enabled in the UI (Interactive Milestone Mode), agents pause after each chunk, report back in IDE chat, and await user instructions. In Continuous Autonomous Swarm Mode, agents claim subsequent chunks immediately.
- **Transparent Activity Heartbeats**: Incoming MCP tool calls automatically refresh your agent session liveness timestamp. No background timer loops are needed.
- **Cryptographic Session Lifetimes**: Session tokens are cryptographically unpredictable with a 30-day sliding activity window and a 365-day maximum lifetime.
- **Final Step Release Delivery**: The agent completing the final step (Step N/N) must build the production bundle (`npm run build` / `cargo build --release`), create an automated launcher script (`run.bat` / `start.sh`) with dependency installation checks and browser auto-launch, verify that the app starts cleanly, and write a full `USER_GUIDE.md` / `HOW_TO_USE.md`.
- **4-Phase Deep Architectural Decomposition**: Plans are organized into 4 full-stack phases (Phase 1 Runnable Baseline & Scaffolding [1-25], Phase 2 Domain Logic & Store Bindings [26-50], Phase 3 High-Fidelity UI Mounted into App.tsx/routes [51-75], Phase 4 Polish, Verification, Launchers & Docs [76-100]) using `append: true`.
- **Real-Time Primary Workspace Synchronization**: Every merged task is automatically synchronized to the primary project directory on disk via `git reset --hard HEAD` and `git clean -fd`.

---

## 8. Complete MCP Tools Reference

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

