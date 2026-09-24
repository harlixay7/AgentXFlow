# Security Policy — AgentXFlow

AgentXFlow is local-first developer infrastructure designed to coordinate multiple autonomous coding agents on the same Git repository safely.

---

## 1. Threat Model

AgentXFlow operates as an authoritative local control plane binding strictly to `127.0.0.1` (loopback only).

### Security Boundaries
- **Loopback Enforcement**: The Model Context Protocol (MCP) daemon binds strictly to `127.0.0.1` by default and validates `Host` and `Origin` headers to prevent unauthorized cross-origin requests from web browsers.
- **Role-Based MCP Authorization**: The coordinator enforces strict caller authority:
  - **Master / Coordinator Authority**: Required for administrative operations including `masterplan_reset`, `masterplan_decompose`, `prepare_masterplan`, `merge_process`, `unclaim_agent_tasks`, `force_agent_idle`, and `task_reconcile`.
  - **Task Ownership Enforcement**: Task mutation tools (`scope_acquire`, `scope_release`, `task_complete_step`, `task_submit`, `task_cancel`, `task_requeue`, `merge_enqueue`) enforce that the authenticated agent is the assigned owner of the target task.
  - **Anti-Impersonation Protection**: Authenticated agents cannot register or send heartbeats on behalf of different agent identities.
- **Dynamic Per-Install & Per-Session Tokens**: On first launch, AgentXFlow generates a cryptographically secure 256-bit authentication token saved in local data storage (`.agentxflow/auth.token`). No hardcoded tokens exist in source code or production builds. All token comparisons use constant-time algorithms (`subtle::constant_time_eq`) to prevent timing side-channel attacks.
- **Zero Self-Certification**: Autonomous agents cannot mark their own criteria valid or bypass verification. MCP requests to self-satisfy criteria (`criteria_satisfy`) are unconditionally rejected.
- **Isolated Git Worktrees**: Agents operate strictly inside dedicated Git worktrees located at `data_dir/AgentXFlow/worktrees/<project>/task-<id>` (primary) or in the coordinator AppData worktree pool. Agents never directly mutate the active working tree or `main` branch.
- **Attempt-Scoped Mutation & Scope Auditing**: On task submission, the coordinator checks `git diff --name-only` and `git status --porcelain` against granted exclusive file locks (`scope_leases`) for the active attempt. Any unreserved file modification triggers a scope violation rejection.
- **Server-Controlled Machine Verification**: An agent cannot mark a task complete by assertion. The coordinator executes verification profiles and machine evaluators (`cargo test`, `npm test`, compiler checks) under its own process supervision with bounded execution timeouts and process-tree termination (`taskkill /T /F`), computing a deterministic SHA-256 evidence digest.
- **Preflight-Safe Destructive Operations**: Masterplan reset operations run an upfront preflight check verifying that all candidate tasks are cancellable before mutating any task state. Explicit plan reset (`masterplan_id`) isolates cancellations strictly to that plan, leaving other plans and worktrees untouched. The user's primary checkout is never modified or hard-reset.
- **Deterministic Security Policy Engine**: Security policies resolve deterministically with most-specific pattern precedence (`longest pattern length wins`) and `id ASC` tie-breaking. Non-negotiable guardrails unconditionally deny destructive commands (`git reset --hard`, `rm -rf /`, `format C:` -> `DENY`; `git push` -> `REQUIRE_APPROVAL`). Unknown policy actions fail closed.
- **Fail-Closed Merge Queue Serialization**: Serialized branch merges enforce strict FIFO order and concurrency checks (maximum 1 integration running checks at any time). If queue state queries fail, the engine fails closed with an error instead of defaulting to zero. Branch updates execute via atomic CAS `git update-ref`.

---

## 2. Token Management & Rotation

- The active authentication token can be inspected in the UI or retrieved via the Tauri IPC `get_mcp_info` command.
- Users can rotate the active token at any time via the UI or `rotate_mcp_token` command.
- Session tokens possess an explicit 365-day maximum lifetime and a 30-day sliding activity renewal window. Active MCP tool calls automatically refresh the sliding window.
- When configuring agents (Google Antigravity, Claude Code, Cursor, OpenCode, OpenAI Codex, Gemini CLI, GitHub Copilot, Windsurf, Junie, Aider), supply the Bearer token in the `Authorization` header:
  ```http
  Authorization: Bearer <your_token>
  ```

---

## 3. Reporting a Vulnerability

If you discover a security vulnerability or bypass in AgentXFlow:
1. Please do **not** open a public GitHub issue.
2. Email security reports directly to the maintainer: **[harlixay7](https://github.com/harlixay7)**.
3. Include a description of the vulnerability, reproduction steps or adversarial test case, and affected versions.
4. We will acknowledge receipt within 48 hours and work with you on a coordinated disclosure timeline.
