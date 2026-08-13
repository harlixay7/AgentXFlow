# AI Agent Setup Guide — AgentXFlow

This document contains step-by-step instructions for AI agents (Claude Code, Antigravity, Cursor, Codex, OpenCode) tasked with setting up, building, and running **AgentXFlow** (by **Viducia**).

Developer: **[harlixay7](https://github.com/harlixay7)**

---

## 1. Check Host Tools

Run these commands first to make sure the machine has what it needs:

```bash
node -v      # Must be v20 or higher
npm -v
cargo -v     # Rust 1.80 or higher
git --version
```

If any tool is missing, stop and inform the user before attempting to build.

---

## 2. Clone & Install Dependencies

```bash
# Clone the repository
git clone https://github.com/harlixay7/AgentXFlow.git
cd AgentXFlow

# On Windows: you can run the automated setup script
setup.bat

# Or run the manual commands:
# 1. Install React/TypeScript dependencies
npm install

# 2. Check frontend types and build bundle
npm run build

# 3. Check Rust backend dependencies
cargo check --manifest-path src-tauri/Cargo.toml
```

---

## 3. Run the Test Suites

Verify that everything compiles and all tests pass:

```bash
# 1. Backend unit tests (state machine, scope engine, merge queue)
cargo test --manifest-path src-tauri/Cargo.toml

# 2. Masterplan decomposition and chunk claim test
cargo test --test masterplan_test --manifest-path src-tauri/Cargo.toml

# 3. Live HTTP MCP server test (all 18 JSON-RPC methods)
cargo test --test mcp_e2e_test --manifest-path src-tauri/Cargo.toml

# 4. Multi-agent concurrent collaboration test (3 agents working in parallel)
cargo test --test multi_agent_concurrent_test --manifest-path src-tauri/Cargo.toml
```

All tests should pass with zero failures.

---

## 4. Starting the App

To run the desktop application and start the local MCP coordination server on `127.0.0.1:7890`:

- On Windows: double click `run.bat`
- Or via terminal:
```bash
npm run tauri dev
```

To run only the web UI preview:

```bash
npm run dev
```

---

## 5. Connecting Yourself to the Coordinator

AgentXFlow runs an HTTP Model Context Protocol (MCP) server at `http://127.0.0.1:7890/mcp`.

### If you are OpenCode
Create `.mcp.json` in the root of the project you want to work on:

```json
{
  "mcpServers": {
    "agentxflow": {
      "url": "http://127.0.0.1:7890/mcp",
      "transport": "http",
      "headers": {
        "Authorization": "Bearer axf_sec_v2_live_token_7890"
      }
    }
  }
}
```

### If you are Claude Code or Codex CLI
Connect via HTTP:
- Endpoint: `http://127.0.0.1:7890/mcp`
- Header: `Authorization: Bearer axf_sec_v2_live_token_7890`

### If you are Antigravity
The coordinator skill specification is located at `SKILL.md`.

---

## 6. How to Work on a Task

When assigned a task:

1. **Claim Task**: Call `task.claim(task_id, agent_id)`. This creates your private git worktree at `.agentxflow/worktrees/task-<id>`.
2. **Lock Files**: Call `scope.acquire(task_id, ["src/your_dir/**"])` before touching any code.
3. **Edit Code**: Make all your changes inside your worktree directory only. Never edit the main workspace directly.
4. **Attach Test Evidence**: Run your unit tests and call `task.complete_step(step_id, evidence)` with the output.
5. **Submit for Review**: Call `task.submit(task_id, agent_id)`. The coordinator will run the tests itself, verify your `git diff` matches your locked files, and place your branch into the merge queue.
