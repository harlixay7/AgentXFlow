# Contributing to AgentXFlow

Thank you for your interest in contributing to AgentXFlow! AgentXFlow is an authoritative local-first coordinator for autonomous AI engineering agents.

---

## 1. Development Environment Setup

### Prerequisites
- **Rust Toolchain**: 1.80+ (`rustup default stable`)
- **Node.js**: 20+ (`npm install -g npm@latest`)
- **Git**: 2.38+ with worktree support

### Quickstart
1. Clone the repository:
   ```bash
   git clone https://github.com/harlixay7/AgentXFlow.git
   cd AgentXFlow
   ```
2. Install frontend dependencies:
   ```bash
   npm install
   ```
3. Run the development environment:
   ```bash
   npm run tauri dev
   ```

---

## 2. Architecture Guidelines & Invariants

AgentXFlow adheres to strict architectural invariants:
- **Authoritative Control Plane**: All file operations, Git mutations, worktrees, and test executions happen through coordinator-supervised Rust backend services.
- **Zero Self-Certification**: Autonomous agents must NEVER self-satisfy criteria or mark their own work valid. Evidence is produced by the agent/worktree, but evaluation and state derivation are strictly performed by the coordinator.
- **Attempt-Scoped Auditing**: Task attempts track mutations, evaluator results, and scope audits. Scope violations belong to specific attempts and clear when covered by acquired leases.
- **Model Context Protocol (MCP) Conformance**: Tools exposed over HTTP strictly adhere to the MCP JSON-RPC 2.0 specification (2024-11-05 standard) and are registered in `src-tauri/src/mcp/registry.rs`.
- **Dynamic Security**: Never hardcode authentication tokens or secrets. Use `SecurityManager` to generate cryptographic tokens stored in local data directories.
- **Adversarial Resilience**: All state transitions, scope reservations, and merge operations must maintain 100% unit and hostile adversarial test coverage.

---

## 3. Running Tests & Quality Gates

Before submitting a pull request, ensure all test suites pass with zero warnings:
```bash
# Run all unit and integration tests
cargo test --manifest-path src-tauri/Cargo.toml

# Run hostile adversarial security & concurrency suite
cargo test --test adversarial_suite_test --manifest-path src-tauri/Cargo.toml

# Run full A-to-Z pipeline test
cargo test --test pipeline_a_to_z_test --manifest-path src-tauri/Cargo.toml

# Run Rust linter (zero warnings allowed)
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# Verify TypeScript and build production bundle
npm run build
```

---

## 4. Code Style & Tone

- Plain-spoken, technical, and systems-engineering focused.
- Zero decorative fluff or filler.
- Emojis are strictly disallowed in all code comments, protocol payloads, and technical markdown docs.
