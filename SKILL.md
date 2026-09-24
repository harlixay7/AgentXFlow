---
name: agentxflow-coordinator
description: Authoritative multi-agent engineering coordinator for parallel workflows with isolated Git worktrees, write scope locking, automated machine verification, and serialized FIFO merge queue.
---

# AgentXFlow Coordinator Skill

You are working under the **AgentXFlow Coordinator** (by **Viducia**).

The coordinator enforces task integrity on the server with deterministic, machine-evaluated quality gates. You cannot mark tasks done in text or merge directly into `main`. You must use AgentXFlow's Model Context Protocol (MCP) tools for your workflow.

---

## 1. Standard Agent Startup Sequence

```
1. Context        -> Call agentxflow_current_context to discover active project, assigned task, and next action.
2. Register       -> Call agent_register(name="<Your_IDE>") with your canonical IDE platform (e.g. "Antigravity", "Claude Code", "Cursor").
3. Contract       -> Call project_context with project_id to fetch architectural rules and conventions.
4. Masterplan     -> Call masterplan_get. Only the currently active/published plan is visible over MCP. If UNSORTED, act as Master Architect: decompose raw specification into high-fidelity steps via masterplan_decompose.
5. Claim Chunk    -> Call masterplan_claim_chunk to allocate an isolated Git worktree (strictly capped by max_steps_per_agent, up to 8 steps).
6. Scope          -> Call scope_acquire with specific file globs before modifying code.
7. Implement      -> Make changes strictly inside your allocated worktree path and verify locally.
8. Step Evidence  -> Call task_complete_step with step_id and command verification evidence.
9. Submit & Gate  -> Call task_submit. The coordinator automatically executes verification profiles, audits scope mutations, generates ProofBundle, and enqueues to merge queue.
10. Milestone Handoff -> Inspect `next_action` in task_submit response:
                     - If `REPORT_TO_USER`: Interactive Milestone mode is active. STOP calling tools immediately, present a milestone walkthrough in chat, and WAIT for user confirmation before claiming the next chunk.
                     - If `masterplan_claim_chunk`: Continuous Autonomous Swarm mode is active. Proceed immediately to claim the next available chunk.
```

---

## 2. Supported Canonical IDE Platforms

When calling `agent_register`, select your canonical platform from the supported roster:

- `Antigravity` (Google Antigravity Advanced Agentic Coding IDE)
- `Claude Code` (Anthropic Claude Code CLI)
- `Cursor` (Cursor AI IDE)
- `OpenCode` (OpenCode Multi-Agent Orchestrator)
- `OpenAI Codex` (OpenAI Codex Agentic Engine)
- `Gemini CLI` (Google Gemini Developer CLI)
- `GitHub Copilot` (GitHub Copilot / VS Code Agent)
- `Windsurf` (Codeium Windsurf AI Cascade IDE)
- `Junie` (JetBrains Junie AI Assistant)
- `Aider` (Aider Pair Programmer CLI)

Registration is completely idempotent—calling with your canonical name always returns the same persistent session and agent ID.

---

## 3. Masterplan Architectural Decomposition Protocol

When `masterplan_get` reports `status: "UNSORTED"`, you are the **Master Architect**. You must generate a production-grade, multi-agent plan with the following strict criteria:

1. **Multi-Masterplan Catalog & Active Plan Isolation**: Projects can contain multiple masterplans (drafts, milestones, epics). Only the masterplan toggled **ON** (`is_active = true`) in the Masterplan Hub is exposed to AI agents via MCP tools (`masterplan_get`, `masterplan_claim_chunk`). If no active masterplan is toggled on, MCP calls return an informative guidance message.
2. **Project-Tailored Folder Structure**: Design a clean, modular directory tree matching the project's actual tech stack (e.g. React/Vite/Tauri/Rust/Node).
3. **Exhaustive Step Specifications (Zero Toy Demos)**:
   - **Target Files**: Explicit relative file paths to create or modify (e.g. `src/components/Navigation/Sidebar.tsx`, `src/types/navigation.ts`).
   - **Concrete Exports & Interfaces**: Specific type definitions, function signatures, state hooks, and API routes to implement.
   - **Professional UX Standard**: Require responsive flex/grid layouts, clean glassmorphism/modern palettes, robust state management, dark/light themes, keyboard shortcuts, and zero placeholder stubs.
   - **Zero Cliché Tropes**: Avoid excessive purple glows or generic vibe fluff; prioritize crisp contrast, high density, and functional excellence.
   - **Non-Overlapping Scopes**: Assign distinct file globs per step (e.g. `src/components/Navigation/**`, `src-tauri/src/db/**`) so multiple agents can work in parallel without lock contention.
   - **Automated Verification Criteria**: Exact machine commands (e.g. `npm run build`, `cargo test --test auth_test`).
4. **Target Step Count**: Decompose into the target step count (configurable up to 100 steps; default 20 steps) to allow maximum parallelization across agents. Anti-hoarding chunk caps support up to 8 steps per agent.

---

## 4. MCP Tools Reference

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
| `masterplan_decompose` | Master | `project_id`, `steps`, `append?`, `idempotency_key?`, `compact?` | Normalize raw masterplan text into structured, non-overlapping execution steps. Supports chunked decomposition with append mode, SHA-256 request hash idempotency checking, crash-atomic storage, and compact range continuation responses (`persisted_range`, `total_known`, `next_expected_range`, `continuation_required`). |
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
| `task_cancel` | Task Owner / Master | `task_id`, `agent_id?`, `reason?` | Cancel an active task, releasing write scope leases, cleaning up worktrees, and reverting incomplete masterplan steps back to PENDING (completed steps remain preserved). |
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

## 5. Execution Rules

1. **Direct Native MCP Transport**: Connect directly to the AgentXFlow MCP server at `http://127.0.0.1:7890/mcp` or use the native Node client helper (`node scripts/agentxflow_client.mjs <command>`). Avoid wrapping large JSON payloads into PowerShell command-line strings.
2. **Persistent MCP Session**: Register once per session (`agent_register`) and reuse your persistent session token. Do not re-register before every status or decomposition call.
3. **Discover Context First**: Call `agentxflow_current_context` to determine current project handoff instructions and active tasks.
4. **Register with Canonical IDE Name**: Always pass your recognized IDE name (e.g. `agent_register(name="Antigravity")`).
5. **Waiting-for-Permission State**: When waiting for user review or IDE confirmation in Interactive Milestone Mode, send `agent_heartbeat` with `waiting_for_permission: true` and `task_id` to extend stale recovery grace from 5 minutes to 30 minutes without holding locks indefinitely.
6. **Resumable Chunked Decomposition**: Large masterplans (e.g. 50-100+ steps) can be decomposed in sequential phases with `append: true`. Identical retries with the same `idempotency_key` return stored steps, while conflicting retries with altered step contents are rejected with `Conflicting retry`.
7. **Atomic Single-Transaction Claims**: `claim_masterplan_chunk` atomically reserves steps, binds task linkage, and updates plan status in a single SQLite transaction, ensuring zero orphaned steps.
8. **Always Pass Exact Project ID**: Never guess project IDs; retrieve exact IDs via `project_list` or `agentxflow_current_context`.
9. **Decompose Unsorted Masterplans Professionally**: When `masterplan_get` reports `status: "UNSORTED"`, formulate full production-grade steps with exact file paths, exports, and scopes before calling `masterplan_decompose`. Returns compact `{ status: "RESORTED", masterplan_id, step_count, pending_steps, next_action }`.
10. **Idempotency & Retry Safety**: Pass an optional `idempotency_key` (e.g. `decompose:<masterplan_id>:<content_hash>`) on `masterplan_decompose` to guarantee safe retries — the server returns the stored decomposition for the same key rather than duplicating or corrupting existing steps.
11. **Respect Chunk Caps**: Claims are strictly capped by `max_steps_per_agent` (up to 8 steps). Never attempt to hoard steps.
12. **Only Edit Locked Files in Worktrees**: The coordinator checks `git diff` against your locked globs. Edit files strictly inside your allocated worktree path. Never modify the repository root directly.
13. **Hybrid Milestone Handoff Compliance**: Inspect `next_action` on `task_submit`:
   - If `next_action == 'REPORT_TO_USER'`: Stop calling tools immediately, report your milestone summary to the user in chat, and wait for confirmation.
   - If `next_action == 'masterplan_claim_chunk'`: Autonomous swarm mode is enabled; proceed to claim your next chunk immediately without waiting.
14. **Automated Machine Verification**: Criteria satisfaction is derived strictly from passing automated machine evaluators and verification profiles. Zero self-certification.
15. **Fix Submission Rejections**: If `task_submit` returns validation errors, inspect `rejection_reasons`, address them inside your worktree, and call `task_submit` again.
16. **Transparent Activity Heartbeats**: Every inbound MCP tool call automatically refreshes your agent heartbeat and active session lease. You do NOT need to run background heartbeat timers unless paused for permission.
17. **Parallel Swarm Collaboration**: Multiple agents can claim distinct chunks and work simultaneously in parallel Git worktrees without lock contention.
18. **Final Step Release Delivery Protocol**: The agent assigned to the final masterplan chunk (containing Step N/N) must deliver the full release:
    - Build the production bundle / executable (`npm run build` / `cargo build --release`).
    - Create a robust, automated launcher script (`run.bat` for Windows / `start.sh` for Unix) that automatically checks and installs dependencies (e.g. `if not exist node_modules call npm install`), starts the server, and auto-opens the browser (`start http://localhost:<port>`).
    - Test and verify that the application launches successfully.
    - Create/update a comprehensive user manual (`USER_GUIDE.md` / `HOW_TO_USE.md`) explaining the full app architecture, configuration, features, navigation map, and step-by-step instructions on how to use the entire application.
    - Present the complete application walkthrough to the user in chat.
19. **4-Phase Deep Architectural Decomposition & Full-Stack Integration**: Masterplans of any target step count N (e.g. 10, 20, 50, 75, 100 steps) are dynamically proportioned into 4 full-stack phases (25% each):
    - Phase 1 (Steps 1 to ~25% N): Runnable Baseline Scaffolding & Core Architecture (Step 1 MUST scaffold the runnable project root: `package.json`, `index.html`, `vite.config.ts`, `main.tsx`/`index.js`, `App.tsx`, and router/navigation skeleton; subsequent Phase 1 steps implement database schemas, shared types, global state stores, and project utilities).
    - Phase 2 (Steps ~26% to ~50% N): Domain Business Logic, State Stores, APIs, Backend Handlers, and Workflows (bound to global app state).
    - Phase 3 (Steps ~51% to ~75% N): High-Fidelity UI Views & Components (MANDATORY: Every component step must specify exact import & mounting instructions in `App.tsx` / `AppRoutes.tsx` / Navigation bar so all features are interactive and visible in the live application—zero isolated/orphaned code).
    - Phase 4 (Steps ~76% to N): Integration, Edge Cases, Verification Suites, Production Launcher Build (`run.bat` / `start.sh`), Launch Validation, and Complete `USER_GUIDE.md` / `HOW_TO_USE.md`.
    - For large plans, you can submit in phased batches using `masterplan_decompose(project_id="...", steps=[...], append=true)` to ensure high-depth specifications.
20. **Preflight-Safe Masterplan Reset & Blast Radius Isolation**: When `masterplan_reset` is invoked, the coordinator executes an upfront preflight check verifying that all candidate tasks are cancellable before mutating any task. An explicit plan reset (`masterplan_id`) cancels only tasks belonging to that plan, prunes Git worktree metadata without wiping unrelated worktrees, and transactionally removes plan records. Project-wide reset (`masterplan_id` omitted) cancels all tasks and cleans `.agentxflow/worktrees`. In both modes, the user's primary checkout is never touched.
21. **Real-Time Primary Working Directory Synchronization**: Every task merge processed by the serialized FIFO merge queue automatically synchronizes the primary repository working directory on disk (`git reset --hard HEAD` and `git clean -fd`), ensuring all merged files immediately appear in the user's workspace in real time.
22. **Role-Based Authorization & Task Ownership Invariant**: The coordinator strictly enforces caller authority. Administrative tools (`masterplan_reset`, `masterplan_decompose`, `prepare_masterplan`, `merge_process`, `unclaim_agent_tasks`, `force_agent_idle`, `task_reconcile`) require Master authority. Task mutations (`scope_acquire`, `scope_release`, `task_complete_step`, `task_submit`, `task_cancel`, `task_requeue`, `merge_enqueue`, `task_workspace_path`, `task_workspace_read`, `task_workspace_write`, `task_workspace_exec`) strictly require the caller to be the assigned owner of the task. Cross-agent impersonation is rejected.
23. **Deterministic Policy Precedence**: Project policies resolve deterministically: when multiple policy rules match a target path or command, the most-specific matching pattern (`longest pattern length wins`) takes precedence. If patterns have equal specificity, deterministic tie-breaking (`id ASC`) resolves the rule. Default policy actions fail closed (`DENY`, `REQUIRE_APPROVAL`, `ALLOW`, with unknown actions producing explicit errors).
24. **Fail-Closed Merge Queue Serialization**: The merge queue guarantees serialized FIFO integration and concurrency locking. If the coordinator cannot authoritatively read earlier READY queue items or active integrations (`RUNNING_CHECKS`), the query fails closed with an error rather than defaulting to zero, preventing race condition bypasses.
25. **Explicit Session Lifetime & Liveness Renewal**: Agent session tokens are cryptographically unpredictable with a 30-day sliding activity window and a 365-day absolute expiration limit. Active tool calls automatically renew the sliding window.
26. **Task-Aware Workspace Proxy Execution**: Agents can perform operations directly inside their isolated worktree without dealing with complex file paths or AppData locations. Use `task_workspace_read` and `task_workspace_write` (which enforces path containment and write scope lease coverage) and `task_workspace_exec` (which executes commands strictly within the worktree with tree-killing on timeout).
27. **Pre-Finalization Target Authority & Stale Base Protection**: Prior to advancing target branches via CAS `update-ref`, the merge engine re-reads the authoritative target SHA on disk. If another merge landed while post-merge verification was running, the candidate is marked `STALE` and the task is transitioned to `BLOCKED` for rebase via `task_reconcile`.
28. **Windows Launcher Reliability Requirements**: When generating `run.bat` for final release delivery, always resolve paths relative to the script directory using `cd /d "%~dp0"`, check errorlevel on installs and builds (`if %errorlevel% neq 0 pause`), start the dev/prod server, and open the browser (`start http://localhost:<port>`).

