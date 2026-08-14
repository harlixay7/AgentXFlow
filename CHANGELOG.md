# Changelog — AgentXFlow

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.0] - 2026-08-14

### Added
- **Core State Engine**:
  - Atomic compare-and-swap task claiming with rollback on worktree creation failures.
  - Strict unidirectional finite state machine (`BACKLOG` -> `READY` -> `RUNNING` -> `REVIEW` -> `MERGE_READY` -> `DONE`).
  - Cumulative anti-hoarding limits on masterplan chunk claims.
  - Dynamic `get_task_details` aggregation.
- **Security & Authorization**:
  - `SecurityManager` with per-install cryptographically random 256-bit authentication tokens.
  - Loopback-only binding enforcement (`127.0.0.1`) with Host and Origin header validation.
  - Token rotation endpoint and UI controls.
- **Atomic File Scoping Engine**:
  - Exclusive write scope leases (`scope_leases`) evaluated atomically in SQLite transactions.
  - Path traversal (`..`) prevention and glob normalization.
  - Git mutation auditing comparing `git status --porcelain` and `git diff --name-only` against active leases.
- **Authoritative Verification Gate**:
  - Process-isolated coordinator test execution inside dedicated Git worktrees.
  - Deterministic SHA-256 evidence bundle generation (`ProofBundle`).
  - Automatic invalidation of stale verifications when worktree HEAD commit moves.
- **Serialized FIFO Merge Queue Engine**:
  - Atomic queue reload by ID preventing stale cache races.
  - Stale target base detection stopping merges if `target_branch` advances.
  - Hidden integration worktree (`.agentxflow/integration`) isolated from user working trees.
  - Atomic reference updating via `git update-ref`.
- **Model Context Protocol (MCP) Gateway**:
  - Standard JSON-RPC 2.0 `initialize`, `tools/list`, and `tools/call` endpoints with full parameter JSON Schemas (2024-11-05 standard, 2026-07-28 negotiation).
  - Streamable HTTP and SSE ping transports.
- **Adversarial Test Suite**:
  - 30-scenario hostile test suite verifying security rejections, race conditions, scope violations, dirty worktrees, and merge conflicts.
