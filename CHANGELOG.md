# Changelog — AgentXFlow

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.5.2] - 2026-09-03

### Concurrency & Isolation
- **Multi-Agent Stale Recovery Isolation**: Resolved canonical agent name vs ID discrepancies in orphaned step detection. Stale recovery sweeps now strictly target the candidate agent, guaranteeing that other active agents remain 100% untouched.
- **Waiting-for-Permission Grace Window**: Added `WAITING_PERMISSION_GRACE = 1800s` (30 minutes) for tasks in `substate = 'WAITING_FOR_INPUT'`, protecting agents awaiting user or IDE input while bounding reclamation.
- **Atomic Single-Transaction Chunk Claims**: `claim_masterplan_chunk` executes step reservation, task creation, and plan status updates inside a single SQLite transaction with fail-closed rollback.
- **Migration 17 (`request_hash`)**: Added SHA-256 hash tracking to `masterplan_operations` for idempotent chunk retries, preventing duplicate or corrupt decompositions.

### Workspace Ergonomics & MCP
- **Task-Aware Workspace Tools**: Added `task_workspace_path`, `task_workspace_read`, `task_workspace_write`, and `task_workspace_exec`. Agents execute commands and edit files directly within their isolated worktree without manually manipulating AppData paths.
- **Path Traversal & Write Scope Security**: All task workspace tools validate caller ownership, enforce worktree root containment against `..` traversals, and verify write-scope lease coverage before writing files.

### Merge Engine & Target Authority
- **Pre-Finalization Target SHA Guard**: Right before executing Compare-and-Swap (CAS) `git update-ref`, the merge engine re-reads the authoritative target branch SHA on disk. If another merge landed while post-merge verification was running, the candidate is marked `STALE` and the task is moved to `BLOCKED` for safe rebase/reconciliation.
- **Post-Merge Verification Profiles**: Added automatic detection for Python projects (`pytest`) and web application runtime smoke testing (`npm run smoke`).

### String Substitution Safety
- **JavaScript Regex Hazard Hardening**: Verified that template replacements and source-to-source substitutions never corrupt text containing `$`, `$&`, `$'`, `$\``, or `</script>`.

### Testing & Infrastructure
- **Test Suite Expansion**: Added 20 new tests covering scope collision matrices, task workspace lifecycle and security, target branch authority guards, and string substitution safety. Full test suite now passes 171+ tests with 0 failures and zero warnings.

---

## [0.5.1] - 2026-09-03

### Safety & Integrity
- **Preflight Masterplan Reset Safety**: `reset_masterplan` validates that all affected candidate tasks are cancellable prior to performing any task state mutations or worktree deletions. If any task cannot be cancelled cleanly, the operation aborts with zero mutations.
- **Blast Radius Isolation**: Target `masterplan_id` resets strictly isolate cancellation and worktree cleanup to the specified masterplan, preserving concurrent masterplans and their active worktrees within the same project.
- **Fail-Closed Cleanup & Pruning**: Propagate errors on managed worktree directory removal and `git worktree prune` rather than ignoring failures.
- **Fail-Closed Repository Initialization**: `init_repo` enforces error propagation on directory creation, Git init, and author identity configuration.

### Merge Queue & Concurrency
- **Fail-Closed Merge Queue Serialization**: Replaced fail-open defaults with strict error propagation when querying earlier READY queue candidates and active integrations (`RUNNING_CHECKS`), preventing race condition bypasses.
- **Serialized FIFO Execution**: Guaranteed strict FIFO ordering in merge queue worker processing.
- **Scheduler Deadlock Prevention**: Explicitly drop coordinator database mutex before invoking asynchronous background sweep tasks.

### Policy Engine
- **Deterministic Policy Specificity**: Overlapping project policy rules resolve deterministically using most-specific matching pattern precedence (`longest pattern length wins`) with `id ASC` tie-breaking.
- **Fail-Closed Policy Actions**: Unrecognized policy actions fail closed with explicit errors. Non-negotiable hardcoded guardrails unconditionally deny destructive commands.

### Security & MCP Authorization
- **Role-Based MCP Authorization**: Separated Master/Coordinator authority from Agent session authority. Administrative tools (`masterplan_reset`, `masterplan_decompose`, `prepare_masterplan`, `merge_process`, `unclaim_agent_tasks`, `force_agent_idle`, `task_reconcile`) require Master authority.
- **Task Ownership Enforcement**: Strict caller ownership enforced on `scope_acquire`, `scope_release`, `task_complete_step`, `task_submit`, `task_cancel`, `task_requeue`, and `merge_enqueue`.
- **Anti-Impersonation Protection**: Authenticated agents are blocked from registering or sending heartbeats on behalf of different agent identities.
- **Explicit Session Lifetimes**: 30-day sliding activity renewal window with an absolute 365-day maximum lifetime.

### Observability & Error Propagation
- **Event Stream Integrity**: Replaced `rows.flatten()` in `get_events_after` with strict error collection, surfacing malformed event row decoding failures.
- **Atomic Project Creation**: Single atomic transaction wrapping project insertion, contract generation, and baseline rule seeding with deterministic rollback on child failure.

### Testing & Infrastructure
- **Headless Runner Identity**: Configured local Git author identity in `init_repo` and test harnesses for headless CI environments.
- **Test Suite Expansion**: Workspace test suite expanded to 151+ tests passing across unit, hostile adversarial, migration upgrade, and end-to-end pipeline suites with zero warnings.

---

## [0.5.0] - 2026-08-31

### Security
- Constant-time master token comparison to prevent timing attacks.
- Unpredictable per-session tokens with automatic rotation on upgrade.
- Heartbeat-based session expiry sliding (30-day window).
- Token rotation exposed in the integrations UI.
- Authenticated MCP sessions with anti-replay protections.

### Safety
- Masterplan reset never touches the user's primary checkout.
- Primary checkout is never hard-reset on merge; fast-forward only when clean.
- Managed-worktree containment enforced before any directory deletion.

### Verification
- Configured timeout is authoritative; reject invalid timeouts.
- Drain pipes during execution to prevent pipe-buffer false timeouts.
- Terminate the whole process tree on timeout.
- Use the reaped exit status for `exit_code` on all platforms.
- Durable structured timeout evidence for cross-platform diagnostics.
- Bound post-merge verification with a timeout and tree kill.

### State Machine
- Centralized validated task transition primitive for all high-risk state writes.
- Route all high-risk task state writes through the validated transition primitive.
- Enforce legal task states at the database layer via CHECK constraints.

### Database
- Migration v12: timeout evidence, append-only proofs, state CHECK constraints, idempotency, token rotation.
- Fail closed when another coordinator instance holds the lock.
- Fresh-install + reopen round-trip and idempotent re-run tests.

### Merge Queue
- Enqueue state write errors propagate to callers.
- Integration worktree reset failures propagate.

### Scope Enforcement
- Audit fails closed on git error instead of passing empty.
- Violation records are authoritative; persistence failures propagate.
- Conservative segment-aware glob overlap detection.
- Prefix-compatible boundary overlap detection.

### Proof Bundles
- Canonical digest covers all evidence deterministically.
- Append-only proof records preserve per-attempt history.
- Merge gate verifies proof hashes before enqueue.

### DAG Scheduling
- `RELATED_TO` is informational; `BLOCKS` and `PARENT_CHILD` gate scheduling.

### MCP Protocol
- JSON-RPC 2.0 structural conformance (`-32700`, `-32600`, `-32601`, `-32602`).
- Advertise and negotiate only the implemented protocol version.
- Remove non-functional SSE stub and de-advertise `sse_url`.
- Enforce task ownership for agent callers.
- Add missing JSON-RPC conformance test cases.
- Dynamically calculate 4-phase step ranges for any arbitrary target step count.

### Masterplan
- Enforce decompose idempotency keys end to end.
- Chunk claims are conditional and atomic.

### Lifecycle
- Unregister performs complete transactional cleanup.
- Periodic stale-recovery sweep with safe transitions.

### Frontend
- Serialize task states as `SCREAMING_SNAKE` to match the UI contract.
- Guard state loads against races and surface poll errors.
- Surface previously silent polling and submission errors.

### Observability
- Emit merge lifecycle events for UI refresh.

### Infrastructure
- Apply `rustfmt` across the crate (baseline quality gate).
- Auto-dependency installation and prerequisite checks in `run.bat`.
- Synchronized version to `0.5.0` across all build artifacts (`package.json`, `Cargo.toml`, `tauri.conf.json`, MCP server info).
- Replaced hardcoded MCP server version with `env!("CARGO_PKG_VERSION")` for single-source-of-truth versioning.
- Synchronize claims and tool reference tables with the hardened implementation.

---

## [0.4.4] - 2026-08-16

### Added
- **Final Release Delivery Protocol (Step N/N)**:
  - Coordinator core detects final chunk completion (`Step N/N`) and returns `next_action: "FINAL_RELEASE_DELIVERY"`.
  - Mandates building the production executable, generating automated launch scripts (`run.bat` for Windows / `start.sh` for Unix), verifying launch, and writing a comprehensive `USER_GUIDE.md` / `HOW_TO_USE.md`.
- **4-Phase Deep Architectural Decomposition & Phased Chunking**:
  - Added `append: true` support to `masterplan_decompose` for phased 25-step chunking across 4 distinct phases (Foundation, Domain Logic, UI & Motion, Polish & Release).
  - Rich prompt guidance in `masterplan_get` instructing architects to expand specifications with creative UX ideas, defensive error boundaries, state flows, and non-overlapping scopes.
- **Clean Repository Reset on Masterplan Reset**:
  - `reset_masterplan` cancels active tasks, wipes worktrees, and clears all step records.
  - Exposed `masterplan_reset` as a native MCP tool (`masterplan_reset(project_id="...", masterplan_id="...")`) with full JSON schema.
- **Real-Time Primary Workspace Synchronization**:
  - Automatically synchronizes the primary repository working directory on disk (`git reset --hard HEAD` and `git clean -fd`) immediately upon merge queue integration, guaranteeing that all merged files appear in the user's workspace in real time.
- **Full-Stack Baseline & Root Mounting Rules**:
  - Architect decomposition now enforces Step 1 runnable baseline scaffolding (`package.json`, `index.html`, `vite.config.ts`, `main.tsx`, `App.tsx`, router) and mandatory mounting of all UI components into `App.tsx`/routes across all phases to eliminate isolated/orphaned code.
- **Masterplan UI Vertical Scrolling**:
  - Wrapped Masterplan Hub catalog and step inspection views in responsive `.view-content` flex scroll viewports for smooth vertical scrolling across any list size.

---

## [0.4.3] - 2026-08-16

### Added
- **Dynamic Agent State Engine**:
  - Automatically derives live agent status (`WORKING`, `IDLE`, `DISCONNECTED`) based on active in-flight task assignments (`RUNNING` / `VERIFYING`) and a 120-second liveness threshold.
  - Adds `active_task_id`, `active_task_title`, and `last_seen_seconds` to the `Agent` model.
- **Transparent MCP Activity Heartbeats**:
  - Automatically touches agent session timestamps and heartbeats on every inbound MCP tool call without requiring agents to run background heartbeat timers.
- **Interactive Step Unclaiming & Worktree Cleanup UI**:
  - **Agent Management View**: Added live color indicators (🟢 IDLE, 🔵 WORKING, ⚪ DISCONNECTED), last seen timers ("Active 5s ago", "Seen 3m ago"), and 1-click **"Unclaim Steps"** & **"Force Idle"** controls.
  - **Masterplan Hub View**: Added agent indicators on claimed chunk cards, inline 1-click **"Unclaim"** buttons for individual steps, and a batch **"Unclaim All (N)"** button to instantly release stale in-flight work back to `PENDING`.
- **Coordinator Engine & IPC Extensions**:
  - Added `unclaim_agent_tasks` and `force_agent_idle` core methods and exposed them via Tauri IPC and MCP tools (`unclaim_agent_tasks`, `force_agent_idle`).

---

## [0.4.2] - 2026-08-15

### Added
- **Compact Response Mode for `masterplan_decompose`**:
  - By default, `masterplan_decompose` returns a token-efficient summary `{ status: "RESORTED", masterplan_id, step_count, pending_steps, next_action }` rather than echoing 75-100 full step objects over MCP.
  - Supports optional `compact: false` parameter for clients requiring full step objects.
- **Decomposition Idempotency & Safe Retry Protection**:
  - Added `idempotency_key` parameter support to `masterplan_decompose`.
  - Added server-side idempotency checks in coordinator core to return existing structured steps on duplicate requests rather than rejecting with hostile active claim conflicts.
- **Native Node.js MCP Client Helper (`scripts/agentxflow_client.mjs`)**:
  - Lightweight, dependency-free native Node client using native `fetch` and `Buffer`.
  - Automatically loads authentication token, registers agent platform once per session, and connects directly to `http://127.0.0.1:7890/mcp`.
  - Eliminates PowerShell command-line argument wrapping and shell bridge latency.

---

## [0.4.1] - 2026-08-15

### Fixed
- **MCP `project_context` Dispatcher Fix for Fresh Agent Workflows**:
  - Resolved workflow bug where calling `project_context` without an optional `task_id` failed by attempting to query a task with an empty string (`""`).
  - Added dedicated `get_project_context(&project_id)` engine method returning `ProjectContextPack` containing project contract hash, overview, memory, and mandatory rules.
  - Retained full task-level context packing when `task_id` is supplied.
  - Added offline SQLite fallback support for `project_context` in `scripts/mcp-bridge.mjs`.
  - Added unit, regression, and E2E MCP test suites for `project_context` with and without `task_id`.

---

## [0.4.0] - 2026-08-15

### Added
- **Multi-Masterplan Catalog & Single-Active Toggle System (Migration 0011)**:
  - Added `title` and `is_active` columns to `masterplans` table with project-active index.
  - Rebuilt Masterplan Hub with a two-level hierarchy: Masterplan Catalog & Selection Grid (manage multiple draft/archived/active plans) and Detailed Plan Workspace.
  - 1-Click Active/Inactive toggle switch: Only active masterplans (`is_active = true`) are visible and actionable to AI agents via MCP.
  - Strict single-active plan mutual exclusion per project: Attempting to activate a plan while another is active triggers a conflict resolution modal with a 1-click switch action.
  - MCP Visibility & Tool Gates: `masterplan_get`, `masterplan_status`, and `masterplan_claim_chunk` query strictly the active plan (`is_active = 1`). Inactive plans are safely sequestered from agent tool calls.
  - Expanded capacity limits: Target step count dropdown expanded up to **100 steps** (5, 10, 15, 20, 25, 30, 40, 50, 60, 75, 100), and chunk anti-hoarding cap expanded up to **8 steps per agent**.
  - High-performance non-blocking query architecture: Resolved internal SQLite connection lock contention and added child process timeout protection to prevent hanging operations.

---

## [0.3.0] - 2026-08-15

### Added
- **Hybrid Milestone Handoff vs Autonomous Swarm Mode (Migration 0010)**:
  - Added `require_milestone_approval` column to `masterplans` with dedicated migration.
  - Interactive UI toggle banner in Masterplan Hub to switch seamlessly between **Interactive Milestone Checkpoints** (pause, report in chat, await confirmation) and **Continuous Autonomous Swarm Mode** (uninterrupted continuous chunk execution).
  - Dynamic `task_submit` MCP response emitting `next_action: "REPORT_TO_USER"` or `next_action: "masterplan_claim_chunk"` accordingly.
- **Canonical AI IDE Identity System (Migration 0009)**:
  - Hardcoded and pre-seeded persistent first-class profiles for all major AI coding platforms: `Antigravity`, `Claude Code`, `Cursor`, `OpenCode`, `OpenAI Codex`, `Gemini CLI`, `GitHub Copilot / VS Code`, `Windsurf`, `Junie`, `Aider`.
  - Upgraded `agent_register` MCP schema with an explicit enum dropdown.
  - Normalized agent registration in coordinator core with alias canonicalization, eliminating random UUID duplication.
- **Masterplan Architectural Decomposition Standard**:
  - Structured guidelines and blueprint templates in `masterplan_get` for `UNSORTED` specifications.
  - Mandated project-tailored folder tree structures, exact target file paths, concrete export/interface declarations, non-overlapping scope patterns, and automated verification commands.
  - Production-grade UI/UX design standards: responsive layouts, clean modern design tokens, zero cliché glowing purple fluff, and zero placeholder stubs.
- **Automated Background Merge Processing & Migration 0007 / 0008**:
  - Continuous asynchronous merge queue worker loops in Tauri application runtime and headless MCP daemon.
  - Forward migrations for proof bundle schema normalization and worktree persistence.
  - Startup database schema integrity verification (`verify_schema_integrity`).

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
