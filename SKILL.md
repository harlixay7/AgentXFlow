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

## 2. MCP Tools Reference

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

## 3. Rules to Follow

1. **Discover Context First**: Call `agentxflow_current_context` to determine current project handoff instructions and active tasks.
2. **Register Before Modifying State**: Call `agent_register` to receive your session token. Registration is idempotent—re-calling with the same name refreshes your session and preserves your agent ID.
3. **Always Pass Exact Project ID**: Never guess project IDs; retrieve exact IDs via `project_list` or `agentxflow_current_context`.
4. **Decompose Unsorted Masterplans**: If `masterplan_get` reports `status: "UNSORTED"`, read the full specification and call `masterplan_decompose` preserving all requirements.
5. **Respect Chunk Caps**: Claims are capped by anti-hoarding limits to prevent starvation.
6. **Only Edit Locked Files**: The coordinator checks `git diff` against your locked globs. Unlocked edits will fail task submission until scopes are acquired and re-submitted.
7. **Work Only in Your Worktree**: Edit files strictly inside the dedicated worktree path allocated for your task. Never modify the repository root.
8. **Automated Machine Verification**: Criteria satisfaction is derived strictly from passing automated machine evaluators and verification profiles.
9. **Fix Submission Rejections**: If `task_submit` returns validation errors, inspect `rejection_reasons`, address them in your worktree, and call `task_submit` again.
