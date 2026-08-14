# Changelog — AgentXFlow

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.2.0] - 2026-08-14

### Added
- **Authoritative Automated Machine Verification Engine**:
  - Replaced manual criteria sign-offs with typed, machine-evaluable checks (`execute_profile_for_attempt`).
  - Automatic detection and execution of project test suites (`Cargo.toml`, `package.json`).
  - Storing typed evaluator results (`evaluator_results`) containing task ID, attempt ID, commit SHA, evaluator version, exit code, execution duration, and stdout/stderr SHA-256 digests.
  - State transition sequence: `RUNNING` -> `VERIFYING` -> `VERIFIED` -> `MERGE_READY` -> `MERGED` (with automatic merge queue auto-enqueue on verification pass).
  - Derived criteria satisfaction strictly from passing machine evaluator results.
- **Zero Self-Certification Gate**:
  - Permanently restricted autonomous agents from calling `criteria_satisfy` over MCP.
- **Task Attempts & Attempt-Scoped Mutation Auditing (Migration 0005)**:
  - Added `task_attempts`, `evaluator_results`, and `verification_profiles` tables with foreign keys and performance indexes.
  - Linked `scope_violations` and `proof_bundles` to active `attempt_id`.
  - Auditing scope mutations per attempt (`audit_attempt_mutations`), allowing agents to acquire missing scope leases and re-run/re-submit with clean violation resolution.
- **Caller Ownership & Step Authorization**:
  - Added caller task ownership validation to `complete_step` (`step.task.assigned_agent == caller_agent`). Non-owners are rejected.
- **Atomic Masterplan Preparation (`prepare_masterplan`)**:
  - Consolidated masterplan saving, raw text markdown/bullet parsing, decomposition into non-overlapping steps, and snapshot generation into a single atomic backend operation.
- **Unified Centralized MCP Registry**:
  - Created `src-tauri/src/mcp/registry.rs` as the single source of truth for all 18+ coordinator tool schemas, descriptions, and parameter definitions.
- **Personalized Context Discovery**:
  - Updated `agentxflow_current_context` to accept `caller_agent_id` and return tailored active task, attempt ID, worktree path, and held scope leases.

---

## [0.1.0] - 2026-08-14

### Added
- **Masterplan Execution Hub & Agent Handoff**:
  - Structured Agent Handoff view displaying exact project identity, masterplan metadata, next required action, and copyable prompt.
  - Live sequence event polling dynamically updating UI on `MASTERPLAN_DECOMPOSED` and chunk claims without manual refresh.
  - Decomposed step checklist with status indicators, assigned agents, and worktree information.
- **Model Context Protocol (MCP) Discovery**:
  - `agentxflow_current_context` endpoint returning active project, masterplan status, and handoff instructions.
  - `project_list` endpoint providing exact IDs, paths, and target branches for deterministic project selection.
  - `masterplan_list` endpoint providing cross-project masterplan overview.
  - Complete project identity block included in `masterplan_get` responses.
  - Required `project_id` parameter validation on `task_list`.
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
  - Disposable integration worktree (`.agentxflow/integration`) isolated from user working trees.
  - Atomic reference updating via `git update-ref`.
- **Adversarial Test Suite**:
  - 30-scenario hostile test suite verifying security rejections, race conditions, scope violations, dirty worktrees, and merge conflicts.
  - 28-step A-to-Z comprehensive end-to-end integration test suite.
