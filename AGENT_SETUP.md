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
# 1. Run all backend unit and integration test suites
cargo test --manifest-path src-tauri/Cargo.toml

# 2. Run complete A-to-Z pipeline integration test
cargo test --test pipeline_a_to_z_test --manifest-path src-tauri/Cargo.toml

# 3. Run 30-scenario hostile adversarial security test suite
cargo test --test adversarial_suite_test --manifest-path src-tauri/Cargo.toml

# 4. Verify code formatting and linting (zero warnings enforced)
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# 5. Type-check frontend bundle
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
10. Milestone Stop-> When task_submit returns CHUNK_COMPLETED, STOP calling tools immediately. Present a milestone summary in chat and WAIT for the user to prompt before claiming the next chunk.
```

### Fundamental Coordination Invariants

- **Zero Self-Certification**: Autonomous agents cannot mark their own criteria valid or bypass verification. All criteria satisfaction is derived exclusively from passing automated machine evaluators executed by the coordinator.
- **Isolated Worktrees**: All code edits must occur inside `.agentxflow/worktrees/task-<id>` or the assigned AppData worktree path. Never edit the primary repository root directly.
- **Attempt-Scoped Auditing**: Scope violations are bound to your active `attempt_id`. Acquiring missing scope leases cleanly clears violations on subsequent re-runs and submissions.
- **Milestone Checkpoints**: Upon chunk submission (`CHUNK_COMPLETED`), the agent stops calling tools and presents a progress report to the user in their IDE chat, waiting for user instructions before claiming subsequent chunks.

---

## 8. Complete MCP Tools Reference

| Tool | Parameters | Description |
|---|---|---|
| `agentxflow_current_context` | `agent_id?`, `project_id?` | Get tailored context, active task, assigned worktree, active scopes, and recommended next action. |
| `project_list` | _(none)_ | List all managed projects with exact IDs, repository paths, and target branches. |
| `project_context` | `project_id`, `task_id?` | Fetch contract hash and project architectural rules. |
| `masterplan_list` | _(none)_ | List all masterplans across all projects with status, step counts, and active handoffs. |
| `masterplan_get` | `project_id` | Inspect masterplan state, raw specification text, project identity, and architect decomposition instructions. |
| `masterplan_status` | `project_id` | Query plan progress stats, total steps, and step statuses. |
| `prepare_masterplan` | `project_id`, `raw_text`, `target_step_count?`, `max_steps_per_agent?` | Atomically save, parse, structure, and prepare a masterplan for agents. |
| `masterplan_decompose` | `project_id`, `steps` | Normalize raw masterplan text into structured, non-overlapping execution steps. |
| `masterplan_claim_chunk`| `project_id`, `agent_id`, `count?` | Claim next batch of steps (strictly capped by limit) and allocate an isolated Git worktree. |
| `agent_register` | `name`, `agent_type` | Idempotently register agent session with a canonical IDE identity and get an authoritative session token. |
| `agent_heartbeat` | `agent_id` | Refresh your session heartbeat and active lease timers. |
| `task_list` | `project_id` | List tasks in the backlog or ready queue for a specific project. |
| `task_get` | `task_id` | Get task prompt, acceptance criteria, and worktree path. |
| `task_details` | `task_id` | Get complete task details including steps, acceptance criteria, active scope leases, attempts, and verification results. |
| `task_claim` | `task_id`, `agent_id` | Claim a task and create an isolated Git worktree on disk with an active task attempt. |
| `scope_acquire` | `task_id`, `agent_id`, `patterns` | Lock file globs (e.g. `['src/auth/**', 'tests/auth_test.rs']`) for exclusive writes. |
| `scope_release` | `task_id`, `agent_id?` | Release held write locks back to the pool. |
| `task_complete_step` | `step_id`, `agent_id?`, `evidence?` | Mark a required task step complete with test output (verifies caller task ownership). |
| `dag_dependencies` | `task_id` | List blocker tasks that must finish before this task starts. |
| `task_submit` | `task_id`, `agent_id` | Submit task; coordinator automatically executes verification profiles, machine evaluators, and git diff mutation audit. Returns milestone handoff instructions on completion. |
| `task_cancel` | `task_id`, `agent_id?`, `reason?` | Cancel an active task, releasing all write scope leases, cleaning up worktrees, and reverting any masterplan steps back to PENDING. |
| `task_requeue` | `task_id`, `agent_id?` | Requeue a claimed chunk task back to masterplan pending steps, releasing held scope leases. |
| `task_reconcile` | `task_id` | Reconcile task state, task attempt, proof bundle, and merge queue status. |
| `merge_queue_status` | `project_id` | Check queue position and status for serialized branch merges. |
| `merge_enqueue` | `project_id`, `task_id` | Enqueue a verified or MERGE_READY task into the serialized merge queue. |
| `merge_process` | `project_id` | Process the next ready serialized branch merge in queue for a project. |
