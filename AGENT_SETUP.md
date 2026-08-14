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

---

## 6. Standard Agent Workflow & Principles

```
1. Context     -> Call agentxflow_current_context to discover active project, assigned task, and next action.
2. Register    -> Call agent_register to obtain your authenticated agent session token (idempotent).
3. Contract    -> Call project_context with project_id to fetch architectural rules.
4. Masterplan  -> Call masterplan_get. If UNSORTED, decompose into structured steps via masterplan_decompose or prepare_masterplan.
5. Claim Chunk -> Call masterplan_claim_chunk or task_claim to allocate an isolated Git worktree.
6. Scope       -> Call scope_acquire with file glob patterns before editing files.
7. Implement   -> Make code changes strictly inside your assigned worktree path and run tests.
8. Evidence    -> Call task_complete_step with step_id, agent_id, and command output.
9. Submit      -> Call task_submit. The coordinator automatically executes verification profiles and audits git mutations.
10. Merge      -> The coordinator transitions verified tasks to MERGE_READY and merges via the serialized merge queue.
```

### Fundamental Coordination Invariants

- **Zero Self-Certification**: Autonomous agents cannot mark their own criteria valid or bypass verification. All criteria satisfaction is derived exclusively from passing automated machine evaluators executed by the coordinator.
- **Isolated Worktrees**: All code edits must occur inside `.agentxflow/worktrees/task-<id>` or the assigned AppData worktree path. Never edit the primary repository root directly.
- **Attempt-Scoped Auditing**: Scope violations are bound to your active `attempt_id`. Acquiring missing scope leases cleanly clears violations on subsequent re-runs and submissions.

---

## 7. Complete MCP Tools Reference

| Tool | Parameters | Description |
|---|---|---|
| `agentxflow_current_context` | `agent_id?`, `project_id?` | Get tailored context, active task, assigned worktree, active scopes, and recommended next action. |
| `project_list` | _(none)_ | List all managed projects with exact IDs, repository paths, and target branches. |
| `project_context` | `project_id`, `task_id?` | Fetch contract hash and project architectural rules. |
| `masterplan_list` | _(none)_ | List all masterplans across all projects with status, step counts, and active handoffs. |
| `masterplan_get` | `project_id` | Inspect masterplan state, raw specification text, project identity, and decomposition instructions. |
| `masterplan_status` | `project_id` | Query plan progress stats, total steps, and step statuses. |
| `prepare_masterplan` | `project_id`, `raw_text`, `target_step_count?`, `max_steps_per_agent?` | Atomically save, parse, structure, and prepare a masterplan for agents. |
| `masterplan_decompose` | `project_id`, `steps` | Normalize raw masterplan text into structured, non-overlapping execution steps. |
| `masterplan_claim_chunk`| `project_id`, `agent_id`, `count?` | Claim next batch of steps (capped by limit) and allocate an isolated Git worktree. |
| `agent_register` | `name`, `agent_type` | Idempotently register agent session and get an authoritative session token and agent_id. |
| `agent_heartbeat` | `agent_id` | Refresh your session heartbeat and active lease timers. |
| `task_list` | `project_id` | List tasks in the backlog or ready queue for a specific project. |
| `task_get` | `task_id` | Get task prompt, acceptance criteria, and worktree path. |
| `task_claim` | `task_id`, `agent_id` | Claim a task and create an isolated Git worktree on disk with an active task attempt. |
| `scope_acquire` | `task_id`, `agent_id`, `patterns` | Lock file globs (e.g. `['src/auth/**', 'tests/auth_test.rs']`) for exclusive writes. |
| `scope_release` | `task_id`, `agent_id?` | Release held write locks back to the pool. |
| `task_complete_step` | `step_id`, `agent_id?`, `evidence?` | Mark a required task step complete with test output (verifies caller task ownership). |
| `dag_dependencies` | `task_id` | List blocker tasks that must finish before this task starts. |
| `task_submit` | `task_id`, `agent_id` | Submit task; coordinator automatically executes verification profiles, machine evaluators, and git diff mutation audit. |
| `merge_queue_status` | `project_id` | Check queue position and status for serialized branch merges. |
