# Security Policy — AgentXFlow

AgentXFlow is local-first developer infrastructure designed to coordinate multiple autonomous coding agents on the same Git repository safely.

---

## 1. Threat Model

AgentXFlow operates as an authoritative local control plane binding to `127.0.0.1` (loopback only).

### Security Boundaries
- **Loopback Enforcement**: The Model Context Protocol (MCP) daemon binds strictly to `127.0.0.1` by default and validates `Host` and `Origin` headers to prevent unauthorized cross-origin requests from web browsers.
- **Dynamic Per-Install Token**: On first launch, AgentXFlow generates a cryptographically secure 256-bit authentication token saved in local data storage (`.agentxflow/auth.token`). No hardcoded tokens exist in source code or production builds.
- **Isolated Git Worktrees**: Agents operate strictly inside dedicated Git worktrees located at `.agentxflow/worktrees/task-<id>`. Agents never directly mutate the active working tree or `main` branch.
- **Mutation & Scope Auditing**: On task submission, the coordinator checks `git diff --name-only` and `git status --porcelain` against granted exclusive file locks (`scope_leases`). Any unreserved file modification triggers a scope violation rejection.
- **Server-Controlled Verification**: An agent cannot mark a task complete by assertion. The coordinator executes test suites (`cargo test`, `npm test`) under its own process supervision and computes a deterministic SHA-256 evidence digest.

---

## 2. Token Management & Rotation

- The active authentication token can be inspected in the UI or retrieved via the Tauri IPC `get_mcp_info` command.
- Users can rotate the active token at any time via the UI or `rotate_mcp_token` command.
- When configuring agents (OpenCode, Claude Code, Antigravity, Codex), supply the Bearer token in the `Authorization` header:
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
