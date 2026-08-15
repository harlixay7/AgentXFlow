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
4. Masterplan     -> Call masterplan_get. If UNSORTED, act as Master Architect: decompose raw specification into high-fidelity steps via masterplan_decompose.
5. Claim Chunk    -> Call masterplan_claim_chunk to allocate an isolated Git worktree (strictly capped by max_steps_per_agent).
6. Scope          -> Call scope_acquire with specific file globs before modifying code.
7. Implement      -> Make changes strictly inside your allocated worktree path and verify locally.
8. Step Evidence  -> Call task_complete_step with step_id and command verification evidence.
9. Submit & Gate  -> Call task_submit. The coordinator automatically executes verification profiles, audits scope mutations, generates ProofBundle, and enqueues to merge queue.
10. Milestone Stop-> When task_submit returns CHUNK_COMPLETED, STOP calling tools immediately. Present a milestone summary in chat and WAIT for the user to prompt before claiming the next chunk.
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

1. **Project-Tailored Folder Structure**: Design a clean, modular directory tree matching the project's actual tech stack (e.g. React/Vite/Tauri/Rust/Node).
2. **Exhaustive Step Specifications (Zero Toy Demos)**:
   - **Target Files**: Explicit relative file paths to create or modify (e.g. `src/components/Navigation/Sidebar.tsx`, `src/types/navigation.ts`).
   - **Concrete Exports & Interfaces**: Specific type definitions, function signatures, state hooks, and API routes to implement.
   - **Professional UX Standard**: Require responsive flex/grid layouts, clean glassmorphism/modern palettes, robust state management, dark/light themes, keyboard shortcuts, and zero placeholder stubs.
   - **Zero Cliché Tropes**: Avoid excessive purple glows or generic vibe fluff; prioritize crisp contrast, high density, and functional excellence.
   - **Non-Overlapping Scopes**: Assign distinct file globs per step (e.g. `src/components/Navigation/**`, `src-tauri/src/db/**`) so multiple agents can work in parallel without lock contention.
   - **Automated Verification Criteria**: Exact machine commands (e.g. `npm run build`, `cargo test --test auth_test`).
3. **Target Step Count**: Decompose into the target step count (default 20 steps) to allow maximum parallelization across agents.

---

## 4. MCP Tools Reference

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

---

## 5. Execution Rules

1. **Discover Context First**: Call `agentxflow_current_context` to determine current project handoff instructions and active tasks.
2. **Register with Canonical IDE Name**: Always pass your recognized IDE name (e.g. `agent_register(name="Antigravity")`).
3. **Always Pass Exact Project ID**: Never guess project IDs; retrieve exact IDs via `project_list` or `agentxflow_current_context`.
4. **Decompose Unsorted Masterplans Professionally**: When `masterplan_get` reports `status: "UNSORTED"`, formulate full production-grade steps with exact file paths, exports, and scopes before calling `masterplan_decompose`.
5. **Respect Chunk Caps**: Claims are strictly capped by `max_steps_per_agent`. Never attempt to hoard steps.
6. **Only Edit Locked Files in Worktrees**: The coordinator checks `git diff` against your locked globs. Edit files strictly inside your allocated worktree path. Never modify the repository root directly.
7. **Milestone Handoff Stop**: When you submit a chunk of steps and receive `status: "CHUNK_COMPLETED"`, STOP calling tools. Output a clear milestone walkthrough to the user in your IDE chat and wait for user confirmation before claiming the next chunk.
8. **Automated Machine Verification**: Criteria satisfaction is derived strictly from passing automated machine evaluators and verification profiles.
9. **Fix Submission Rejections**: If `task_submit` returns validation errors, inspect `rejection_reasons`, address them inside your worktree, and call `task_submit` again.
