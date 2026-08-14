# AI Agent Setup Guide — AgentXFlow

This document contains step-by-step instructions for AI agents (Claude Code, Antigravity, Cursor, Codex, OpenCode) tasked with setting up, building, and interacting with **AgentXFlow** (by **Viducia**).

Developer: **[harlixay7](https://github.com/harlixay7)**

---

## 1. Check Host Prerequisites

Verify host development tools before running builds:

```bash
node -v      # Node.js v20 or higher
npm -v
cargo -v     # Rust 1.80 or higher
git --version
```

If any prerequisite tool is missing, stop and inform the user.

---

## 2. Clone & Install Dependencies

```bash
# Clone the repository
git clone https://github.com/harlixay7/AgentXFlow.git
cd AgentXFlow

# On Windows: run automated setup script
setup.bat

# Or execute manual setup steps:
# 1. Install frontend dependencies
npm install

# 2. Check TypeScript types and build frontend assets
npm run build

# 3. Check Rust backend compilation
cargo check --manifest-path src-tauri/Cargo.toml
```

---

## 3. Run Quality Verification Gates

Verify that all test suites compile and pass 100%:

```bash
# 1. Run all backend unit and integration test suites
cargo test --manifest-path src-tauri/Cargo.toml

# 2. Run complete A-to-Z pipeline integration test
cargo test --test pipeline_a_to_z_test --manifest-path src-tauri/Cargo.toml

# 3. Run 30-scenario hostile adversarial security test suite
cargo test --test adversarial_suite_test --manifest-path src-tauri/Cargo.toml

# 4. Verify code formatting and linting
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# 5. Type-check frontend bundle
npm run build
```

---

## 4. Starting the Coordinator Application

To start the desktop application and boot the local Model Context Protocol (MCP) coordination server on `127.0.0.1:7890`:

- On Windows: double-click `run.bat`
- Or via terminal:
```bash
npm run tauri dev
```

---

## 5. Connecting AI Agents via MCP

AgentXFlow hosts an MCP server conforming to standard JSON-RPC 2.0 at `http://127.0.0.1:7890/mcp`.

Copy your active authentication token from the **MCP Gateway** tab in the desktop application.

### OpenCode (`.mcp.json`)
```json
{
  "mcpServers": {
    "agentxflow": {
      "url": "http://127.0.0.1:7890/mcp",
      "transport": "http",
      "headers": {
        "Authorization": "Bearer <YOUR_COORDINATOR_TOKEN>"
      }
    }
  }
}
```

### Cursor (`.cursor/mcp.json`)
```json
{
  "mcpServers": {
    "agentxflow": {
      "url": "http://127.0.0.1:7890/mcp",
      "headers": {
        "Authorization": "Bearer <YOUR_COORDINATOR_TOKEN>"
      }
    }
  }
}
```

### Claude Code / Claude Desktop / Codex CLI
- Endpoint: `http://127.0.0.1:7890/mcp`
- Header: `Authorization: Bearer <YOUR_COORDINATOR_TOKEN>`

### Antigravity
The canonical coordinator skill definition is located at [`SKILL.md`](SKILL.md).

---

## 6. Standard Agent Workflow

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
