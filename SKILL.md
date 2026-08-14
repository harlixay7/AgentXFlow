---
name: agentxflow-coordinator
description: Authoritative multi-agent engineering coordinator for parallel workflows with isolated Git worktrees, write scope locking, automated verification, and serialized FIFO merge queue.
---

# AgentXFlow Coordinator Skill

You are working under the **AgentXFlow Coordinator** (by **Viducia**).

The coordinator enforces task integrity on the server. You cannot mark tasks done in text or merge directly into `main`. You must use AgentXFlow's Model Context Protocol (MCP) tools for your workflow.

---

## 1. Standard Agent Startup Sequence

```
1. Context     -> Call agentxflow_current_context to discover active project and handoff instructions.
2. Register    -> Call agent_register to obtain your authenticated agent session token.
3. Contract    -> Call project_context with project_id to fetch architectural rules.
4. Masterplan  -> Call masterplan_get. If UNSORTED, decompose into structured steps via masterplan_decompose.
5. Discover    -> Call task_list or masterplan_claim_chunk to claim work batches.
6. Lock        -> Call scope_acquire with file glob patterns before editing files.
7. Code        -> Make changes inside your assigned worktree path and run tests.
8. Evidence    -> Call task_complete_step with command output from your tests.
9. Submit      -> Call task_submit. The coordinator verifies tests and git diffs.
10. Merge      -> The coordinator integrates your branch via the serialized merge queue.
```

---

## 2. MCP Tools Reference

| Tool | Parameters | Description |
|---|---|---|
| `agentxflow_current_context` | _(none)_ | Get the most recently prepared handoff, active project, and recommended next action. |
| `project_list` | _(none)_ | List all managed projects with exact IDs, repository paths, and target branches. |
| `project_context` | `project_id`, `task_id` | Fetch contract hash and project architectural rules. |
| `masterplan_list` | _(none)_ | List all masterplans across all projects with status, step counts, and active handoffs. |
| `masterplan_get` | `project_id` | Inspect masterplan state, raw specification text, project identity, and decomposition instructions. |
| `masterplan_status` | `project_id` | Query plan progress stats, total steps, and step statuses. |
| `masterplan_decompose` | `project_id`, `steps` | Normalize raw masterplan text into structured, non-overlapping execution steps. |
| `masterplan_claim_chunk`| `project_id`, `agent_id`, `count` | Claim next batch of steps (capped by limit) and allocate an isolated Git worktree. |
| `agent_register` | `name`, `agent_type` | Register your AI agent session and get an authoritative session token and agent_id. |
| `agent_heartbeat` | `agent_id` | Refresh your session heartbeat and active lease timers. |
| `task_list` | `project_id` | List tasks in the backlog or ready queue for a specific project. |
| `task_get` | `task_id` | Get task prompt, acceptance criteria, and worktree path. |
| `task_claim` | `task_id`, `agent_id` | Claim a task and create an isolated Git worktree on disk. |
| `scope_acquire` | `task_id`, `patterns` | Lock file globs (e.g. `src/auth/**`) for exclusive writes. |
| `scope_release` | `task_id` | Release held write locks back to the pool. |
| `task_complete_step` | `step_id`, `evidence` | Mark a required task step complete with test output. |
| `dag.dependencies` | `task_id` | List blocker tasks that must finish before this task starts. |
| `task_submit` | `task_id`, `agent_id` | Submit task; coordinator runs checks and verifies git mutations. |
| `merge_queue_status` | `project_id` | Check queue position for pending branch merges. |

---

## 3. Rules to Follow

1. **Discover Context First**: Call `agentxflow_current_context` to determine current project handoff instructions.
2. **Register Before Modifying State**: Call `agent_register` to receive your session token.
3. **Always Pass Exact Project ID**: Never guess project IDs; retrieve exact IDs via `project_list` or `agentxflow_current_context`.
4. **Decompose Unsorted Masterplans**: If `masterplan_get` reports `status: "UNSORTED"`, read the full specification and call `masterplan_decompose` preserving all requirements.
5. **Respect Chunk Caps**: Claims are capped by anti-hoarding limits to prevent starvation.
6. **Only Edit Locked Files**: The coordinator checks `git diff` against your locked globs. Unlocked edits will fail task submission.
7. **Work Only in Your Worktree**: Edit files strictly inside the dedicated worktree path allocated for your task. Never modify the repository root.
8. **Include Real Test Output in Steps**: Attach stdout and exit codes when calling `task_complete_step`.
9. **Fix Submission Rejections**: If `task_submit` returns validation errors, inspect `rejection_reasons`, address them in your worktree, and call `task_submit` again.
