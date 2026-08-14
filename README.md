# AgentXFlow

**AgentXFlow** is an authoritative desktop application and Model Context Protocol (MCP) coordination daemon that enables multiple AI coding agents to collaborate concurrently on a single Git repository through isolated worktrees, write scope leases, automated machine verification gates, and a serialized FIFO merge queue.

Developed by **[harlixay7](https://github.com/harlixay7)** • **AgentXFlow by Viducia**

---

## Core Capabilities

- **Isolated Git Worktrees**: When an agent claims a task or masterplan chunk, AgentXFlow allocates an isolated Git worktree on disk. Agents work exclusively within their assigned worktree branch.
- **Write Scope Leases**: Agents declare intended file glob patterns (e.g. `src/auth/**`) before modifying files. Overlapping write patterns are detected and rejected.
- **Attempt-Scoped Mutation Auditing**: On task submission, the coordinator audits `git diff` against active scope leases for the current attempt. Acquiring missing scope leases cleanly resolves unreserved file violations on re-run.
- **Automated Machine Verification Gates**: The coordinator automatically executes verification profiles and machine evaluators (e.g. cargo test, npm test, type checks, lint checks) directly in the worktree. Criteria satisfaction is derived strictly from passing evaluator results (zero autonomous self-certification).
- **Cryptographic Proof Bundles**: Verified submissions generate an immutable `ProofBundle` sealed with a SHA-256 digest over task metadata, file diffs, and test outputs.
- **Serialized FIFO Merge Queue**: Verified candidate branches are integrated sequentially inside disposable integration worktrees, validated with post-merge tests, and advanced via atomic Compare-and-Swap (CAS) `git update-ref` operations.
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
|   - SQLite Database (Versioned migrations 1-5, WAL mode)      |
|   - Scope Engine (Multi-glob splitting & attempt auditing)    |
|   - Verification Engine (Machine evaluators & profile runner) |
|   - Merge Queue Engine (Serialized FIFO 3-way integration)    |
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

---

## MCP Tools Reference

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
| `task_cancel` | `task_id`, `agent_id?`, `reason?` | Cancel an active task, releasing all write scope leases, cleaning up worktrees, and reverting any masterplan steps back to PENDING. |
| `task_requeue` | `task_id`, `agent_id?` | Requeue a claimed chunk task back to masterplan pending steps, releasing held scope leases. |
| `merge_queue_status` | `project_id` | Check queue position and status for serialized branch merges. |

---

## Testing & Quality Gates

```bash
# Run all backend unit and integration test suites
cargo test --manifest-path src-tauri/Cargo.toml

# Run the complete A-to-Z pipeline integration test
cargo test --test pipeline_a_to_z_test --manifest-path src-tauri/Cargo.toml

# Run the 30-scenario adversarial security and concurrency test suite
cargo test --test adversarial_suite_test --manifest-path src-tauri/Cargo.toml

# Run Rust linter with zero warnings allowed
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# Type-check and build production frontend bundle
npm run build
```

---

## Repository Structure

- `src-tauri/src/core/`: Coordinator engine, state machines, and Tauri IPC commands.
- `src-tauri/src/mcp/`: Model Context Protocol (MCP 2024-11-05) Axum HTTP server with session derivation and tool registry.
- `src-tauri/src/scope/`: Glob pattern collision detection, pattern normalization, and attempt-scoped mutation auditor.
- `src-tauri/src/verification/`: Verification profiles, automated machine evaluators, and SHA-256 proof bundle generator.
- `src-tauri/src/merge/`: Serialized FIFO merge engine with disposable integration worktrees and CAS ref updates.
- `src-tauri/src/db/`: Versioned SQLite migrations (1–6), connection pooling, and single-instance file lock.
- `src/`: React 19 / TypeScript workbench UI.

---

## License

MIT ([`LICENSE`](LICENSE))
