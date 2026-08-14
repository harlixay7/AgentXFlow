---
name: agentxflow-coordinator
description: Authoritative cross-agent engineering coordinator for multi-agent workflows with isolated Git worktrees, write scope locking, test verification, and serialized merge queue.
---

# AgentXFlow Coordinator Skill

You are working under the **AgentXFlow Coordinator** (by **Viducia**, developed by **harlixay7**).

The coordinator enforces task rules on the server. You cannot mark tasks done in text or merge directly into `main`. You must use AgentXFlow's Model Context Protocol (MCP) tools for your entire workflow.

---

## 1. Workflow Steps

```
1. Register    -> Call agent.register to obtain your unique agent_id session.
2. Masterplan  -> Call masterplan.get. If UNSORTED, decompose into structured steps.
3. Discover    -> Call task.list or masterplan.claim_chunk to claim work batches.
4. Lock        -> Call scope.acquire with file glob patterns before editing files.
5. Code        -> Make changes inside .agentxflow/worktrees/task-<id> and run tests.
6. Evidence    -> Call task.complete_step with command output from your tests.
7. Submit      -> Call task.submit. The coordinator verifies tests and git diffs.
8. Merge       -> The coordinator integrates your branch via the merge queue.
```

---

## 2. MCP Tools Reference

| Tool | Parameters | Description |
|---|---|---|
| `agent.register` | `name`, `agent_type` | Register your AI agent session and get an authoritative agent_id. |
| `agent.heartbeat` | `agent_id` | Refresh your session heartbeat and active lease timers. |
| `masterplan.get` | `project_id` | Inspect masterplan state, raw specification text, and decomposition instructions. |
| `masterplan.status` | `project_id` | Query plan progress stats, total steps, and step statuses. |
| `masterplan.decompose` | `project_id`, `steps` | Normalize raw masterplan text into structured, non-overlapping execution steps. |
| `masterplan.claim_chunk`| `project_id`, `agent_id`, `count` | Claim next batch of steps (capped by max limit) and cut a Git worktree. |
| `task.list` | `project_id`, `state` | List tasks in the backlog or ready queue. |
| `task.get` | `task_id` | Get task prompt, acceptance criteria, and worktree path. |
| `task.claim` | `task_id`, `agent_id` | Claim a task and create an isolated Git worktree on disk. |
| `project.context` | `project_id`, `task_id` | Fetch contract hash and project architectural rules. |
| `scope.acquire` | `task_id`, `patterns` | Lock file globs (e.g. `src/auth/**`) for exclusive writes. |
| `scope.release` | `task_id` | Release held write locks back to the pool. |
| `task.complete_step` | `step_id`, `evidence` | Mark a required task step complete with test output. |
| `dag.dependencies` | `task_id` | List blocker tasks that must finish before this task starts. |
| `task.submit` | `task_id`, `agent_id` | Submit task; coordinator runs tests and checks git diff. |
| `merge.queue_status` | `project_id` | Check queue position for pending branch merges. |

---

## 3. Rules to Follow

1. **Register Before Modifying State**: Always call `agent.register` first to receive your `agent_id`.
2. **Decompose Unsorted Masterplans**: If `masterplan.get` reports `status: "UNSORTED"`, read the full raw text and call `masterplan.decompose` without omitting any requirements.
3. **Respect Chunk Caps**: Claims are capped to `max_steps_per_agent` to prevent hoarding.
4. **Only Edit Locked Files**: The coordinator checks `git diff` against your locked globs. Unlocked edits will fail task submission.
5. **Work Only in Your Worktree**: Edit files in `.agentxflow/worktrees/task-<id>`. Never modify the repository root.
6. **Include Real Test Output in Steps**: Attach real stdout and exit codes when calling `task.complete_step`.
7. **Fix Submission Rejections**: If `task.submit` fails, check `rejection_reasons`, fix the code in your worktree, and call `task.submit` again.
