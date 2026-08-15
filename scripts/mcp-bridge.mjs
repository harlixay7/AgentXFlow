#!/usr/bin/env node
/**
 * AgentXFlow Model Context Protocol (MCP) Stdio Gateway
 * Connects Antigravity and other MCP clients to the AgentXFlow Coordinator daemon (http://127.0.0.1:7890/mcp).
 * Automatically reads the authenticated session token from %APPDATA%/agentxflow/.agentxflow/auth.token.
 * When the daemon is offline, provides fast direct read access to agentxflow_v2.db.
 */

import * as readline from 'node:readline';
import * as path from 'node:path';
import * as os from 'node:os';
import * as fs from 'node:fs';
import { DatabaseSync } from 'node:sqlite';

const MCP_PORT = 7890;
const MCP_URL = `http://127.0.0.1:${MCP_PORT}/mcp`;

const ROAMING_DIR = process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming');
const APPDATA_DIR = fs.existsSync(path.join(ROAMING_DIR, 'AgentXFlow'))
  ? path.join(ROAMING_DIR, 'AgentXFlow')
  : path.join(ROAMING_DIR, 'agentxflow');

const TOKEN_PATH = path.join(APPDATA_DIR, '.agentxflow', 'auth.token');
const DB_PATH = path.join(APPDATA_DIR, 'agentxflow_v2.db');

function getAuthToken() {
  try {
    if (fs.existsSync(TOKEN_PATH)) {
      return fs.readFileSync(TOKEN_PATH, 'utf8').trim();
    }
  } catch {}
  return '';
}

function getDirectDb() {
  if (fs.existsSync(DB_PATH)) {
    return new DatabaseSync(DB_PATH, { open: true, readOnly: true });
  }
  return null;
}

const TOOLS = [
  {
    name: 'agentxflow_current_context',
    description: 'Get the most recently prepared handoff, active project, and recommended next action for newly connected agents.',
    inputSchema: { type: 'object', properties: {} },
  },
  {
    name: 'project_list',
    description: 'List all managed projects with their exact project IDs, repository paths, and target branches.',
    inputSchema: { type: 'object', properties: {} },
  },
  {
    name: 'project_context',
    description: 'Get architectural rules, contract hashes, and project metadata.',
    inputSchema: {
      type: 'object',
      properties: {
        project_id: { type: 'string', description: 'Target project ID' },
        task_id: { type: 'string', description: 'Optional task ID' },
      },
      required: ['project_id'],
    },
  },
  {
    name: 'masterplan_list',
    description: 'List all masterplans across all projects with status, step counts, and active handoffs.',
    inputSchema: { type: 'object', properties: {} },
  },
  {
    name: 'masterplan_get',
    description: 'Get masterplan specification, current status, project identity, and decomposition instructions.',
    inputSchema: {
      type: 'object',
      properties: {
        project_id: { type: 'string', description: 'Target project ID' },
      },
      required: ['project_id'],
    },
  },
  {
    name: 'masterplan_status',
    description: 'Query plan progress stats, total steps, and step statuses.',
    inputSchema: {
      type: 'object',
      properties: {
        project_id: { type: 'string', description: 'Target project ID' },
      },
      required: ['project_id'],
    },
  },
  {
    name: 'masterplan_decompose',
    description: 'Decompose raw masterplan into structured execution steps.',
    inputSchema: {
      type: 'object',
      properties: {
        project_id: { type: 'string', description: 'Project ID' },
        steps: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              step_index: { type: 'integer' },
              title: { type: 'string' },
              description: { type: 'string' },
              suggested_scope: { type: 'string' },
              acceptance_criteria: { type: 'string' },
            },
            required: ['step_index', 'title', 'description'],
          },
        },
      },
      required: ['project_id', 'steps'],
    },
  },
  {
    name: 'masterplan_claim_chunk',
    description: 'Claim the next batch of steps from an organized masterplan and allocate an isolated Git worktree.',
    inputSchema: {
      type: 'object',
      properties: {
        project_id: { type: 'string', description: 'Project ID' },
        agent_id: { type: 'string', description: 'Claiming agent ID' },
        count: { type: 'integer', description: 'Optional step count (capped by limit)' },
      },
      required: ['project_id', 'agent_id'],
    },
  },
  {
    name: 'agent_register',
    description: 'Register an agent session with a canonical AI IDE platform and get an authoritative session token and agent_id.',
    inputSchema: {
      type: 'object',
      properties: {
        name: {
          type: 'string',
          enum: [
            'Antigravity',
            'Claude Code',
            'Cursor',
            'OpenCode',
            'OpenAI Codex',
            'Gemini CLI',
            'GitHub Copilot',
            'Windsurf',
            'Junie',
            'Aider',
          ],
          description: 'Select your AI IDE / Agent platform',
        },
        agent_type: {
          type: 'string',
          enum: ['IDE', 'CLI', 'Autonomous Swarm', 'Reviewer', 'Implementer'],
          description: 'Agent category type',
        },
      },
      required: ['name'],
    },
  },
  {
    name: 'agent_heartbeat',
    description: 'Keep agent session and active scope leases alive.',
    inputSchema: {
      type: 'object',
      properties: {
        agent_id: { type: 'string', description: 'Unique agent identifier' },
      },
      required: ['agent_id'],
    },
  },
  {
    name: 'task_list',
    description: 'List all tasks in backlog or ready queue for a project.',
    inputSchema: {
      type: 'object',
      properties: {
        project_id: { type: 'string', description: 'Target project ID' },
      },
      required: ['project_id'],
    },
  },
  {
    name: 'task_get',
    description: 'Get task details including prompt, status, and worktree path.',
    inputSchema: {
      type: 'object',
      properties: {
        task_id: { type: 'string', description: 'Task identifier' },
      },
      required: ['task_id'],
    },
  },
  {
    name: 'task_claim',
    description: 'Atomically claim a task and cut an isolated Git worktree on disk.',
    inputSchema: {
      type: 'object',
      properties: {
        task_id: { type: 'string', description: 'Task identifier' },
        agent_id: { type: 'string', description: 'Claiming agent ID' },
      },
      required: ['task_id', 'agent_id'],
    },
  },
  {
    name: 'scope_acquire',
    description: 'Atomically lock file glob patterns for exclusive write access.',
    inputSchema: {
      type: 'object',
      properties: {
        task_id: { type: 'string', description: 'Task identifier' },
        agent_id: { type: 'string', description: 'Agent identifier' },
        patterns: {
          type: 'array',
          items: { type: 'string' },
          description: 'File glob patterns (e.g. ["src/auth/**", "tests/auth_test.rs"])',
        },
      },
      required: ['task_id', 'agent_id', 'patterns'],
    },
  },
  {
    name: 'scope_release',
    description: 'Release held write locks back to the pool.',
    inputSchema: {
      type: 'object',
      properties: {
        task_id: { type: 'string', description: 'Task identifier' },
      },
      required: ['task_id'],
    },
  },
  {
    name: 'task_complete_step',
    description: 'Mark a required task step completed with test or build evidence.',
    inputSchema: {
      type: 'object',
      properties: {
        step_id: { type: 'string', description: 'Step identifier' },
        evidence: { type: 'string', description: 'Structured command output or test log' },
      },
      required: ['step_id'],
    },
  },
  {
    name: 'dag_dependencies',
    description: 'List blocker tasks that must finish before this task starts.',
    inputSchema: {
      type: 'object',
      properties: {
        task_id: { type: 'string', description: 'Task identifier' },
      },
      required: ['task_id'],
    },
  },
  {
    name: 'task_submit',
    description: 'Submit task for coordinator verification and git mutation audit.',
    inputSchema: {
      type: 'object',
      properties: {
        task_id: { type: 'string', description: 'Task identifier' },
        agent_id: { type: 'string', description: 'Agent identifier' },
      },
      required: ['task_id', 'agent_id'],
    },
  },
  {
    name: 'prepare_masterplan',
    description: 'Atomically save, parse, structure, and prepare a masterplan for agents.',
    inputSchema: {
      type: 'object',
      properties: {
        project_id: { type: 'string', description: 'Project ID' },
        raw_text: { type: 'string', description: 'Raw masterplan text' },
        target_step_count: { type: 'integer', description: 'Target step count' },
        max_steps_per_agent: { type: 'integer', description: 'Max steps per agent' },
      },
      required: ['project_id', 'raw_text'],
    },
  },
  {
    name: 'task_details',
    description: 'Get complete task details including steps, acceptance criteria, active scope leases, attempts, and verification results.',
    inputSchema: {
      type: 'object',
      properties: {
        task_id: { type: 'string', description: 'Task identifier' },
      },
      required: ['task_id'],
    },
  },
  {
    name: 'task_cancel',
    description: 'Cancel an active task, releasing all write scope leases, cleaning up worktrees, and reverting any masterplan steps back to PENDING.',
    inputSchema: {
      type: 'object',
      properties: {
        task_id: { type: 'string', description: 'Task identifier' },
        agent_id: { type: 'string', description: 'Optional agent identifier' },
        reason: { type: 'string', description: 'Cancellation reason' },
      },
      required: ['task_id'],
    },
  },
  {
    name: 'task_requeue',
    description: 'Requeue a claimed chunk task back to masterplan pending steps, releasing held scope leases.',
    inputSchema: {
      type: 'object',
      properties: {
        task_id: { type: 'string', description: 'Task identifier' },
        agent_id: { type: 'string', description: 'Optional agent identifier' },
      },
      required: ['task_id'],
    },
  },
  {
    name: 'task_reconcile',
    description: 'Reconcile task state, task attempt, proof bundle, and merge queue status.',
    inputSchema: {
      type: 'object',
      properties: {
        task_id: { type: 'string', description: 'Task identifier' },
      },
      required: ['task_id'],
    },
  },
  {
    name: 'merge_queue_status',
    description: 'List all queued branch merges and their integration statuses.',
    inputSchema: {
      type: 'object',
      properties: {
        project_id: { type: 'string', description: 'Project ID' },
      },
      required: ['project_id'],
    },
  },
  {
    name: 'merge_enqueue',
    description: 'Enqueue a verified or MERGE_READY task into the serialized merge queue.',
    inputSchema: {
      type: 'object',
      properties: {
        project_id: { type: 'string', description: 'Project ID' },
        task_id: { type: 'string', description: 'Task identifier' },
      },
      required: ['project_id', 'task_id'],
    },
  },
  {
    name: 'merge_process',
    description: 'Process the next ready serialized branch merge in queue for a project.',
    inputSchema: {
      type: 'object',
      properties: {
        project_id: { type: 'string', description: 'Project ID' },
      },
      required: ['project_id'],
    },
  },
];

function getAgentSessionToken(agentId) {
  try {
    const db = getDirectDb();
    if (db && agentId) {
      const row = db.prepare('SELECT session_token FROM agents WHERE id = ?').get(agentId);
      if (row && row.session_token) return row.session_token;
    }
  } catch {}
  return null;
}

async function callHttpDaemon(method, params) {
  let token = null;
  if (params && params.agent_id) {
    token = getAgentSessionToken(params.agent_id);
  }
  if (!token) {
    token = getAuthToken();
  }
  const res = await fetch(MCP_URL, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': token ? `Bearer ${token}` : '',
      'MCP-Protocol-Version': '2024-11-05',
    },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: Date.now(),
      method: 'tools/call',
      params: {
        name: method,
        arguments: params || {},
      },
    }),
  });

  if (!res.ok) {
    throw new Error(`AgentXFlow Daemon HTTP error ${res.status}: ${await res.text()}`);
  }

  const json = await res.json();
  if (json.error) {
    throw new Error(`[${json.error.code}] ${json.error.message}`);
  }
  return json.result;
}

function handleOfflineRead(toolName, args) {
  const db = getDirectDb();
  if (!db) {
    throw new Error('AgentXFlow daemon is not running (127.0.0.1:7890) and no local database found. Start AgentXFlow via run.bat.');
  }

  switch (toolName) {
    case 'project_list': {
      const rows = db.prepare('SELECT id, name, root_path, target_branch, created_at FROM projects').all();
      return rows;
    }
    case 'masterplan_list': {
      const plans = db.prepare('SELECT p.id as project_id, p.name, m.status, m.version FROM projects p LEFT JOIN masterplans m ON m.project_id = p.id').all();
      return plans;
    }
    case 'masterplan_get': {
      const plan = db.prepare('SELECT * FROM masterplans WHERE project_id = ?').get(args.project_id);
      const steps = db.prepare('SELECT * FROM masterplan_steps WHERE project_id = ? ORDER BY step_index ASC').all(args.project_id);
      return { plan, steps };
    }
    case 'task_list': {
      const tasks = db.prepare('SELECT * FROM tasks WHERE project_id = ? ORDER BY created_at DESC').all(args.project_id);
      return tasks;
    }
    case 'task_get': {
      const task = db.prepare('SELECT * FROM tasks WHERE id = ?').get(args.task_id);
      const steps = db.prepare('SELECT * FROM task_steps WHERE task_id = ? ORDER BY sequence_order ASC').all(args.task_id);
      const criteria = db.prepare('SELECT * FROM acceptance_criteria WHERE task_id = ?').all(args.task_id);
      return { task, steps, criteria };
    }
    case 'agentxflow_current_context': {
      const activeProject = db.prepare('SELECT * FROM projects ORDER BY updated_at DESC LIMIT 1').get();
      return {
        active_project: activeProject,
        notice: 'AgentXFlow daemon is currently offline. Direct read access active. Run run.bat to enable worktree allocation, scope locks, and automated verification.',
      };
    }
    default:
      throw new Error(`Mutation tool '${toolName}' requires the live AgentXFlow coordinator. Please start AgentXFlow via run.bat (http://127.0.0.1:7890).`);
  }
}

async function handleToolCall(name, args) {
  try {
    const res = await callHttpDaemon(name, args);
    return res;
  } catch (err) {
    // If daemon is not running, fallback to offline DB read for read-only tools
    try {
      const offlineRes = handleOfflineRead(name, args);
      return {
        content: [
          {
            type: 'text',
            text: JSON.stringify(offlineRes, null, 2),
          },
        ],
      };
    } catch {
      throw err;
    }
  }
}

// JSON-RPC stdio protocol handler
const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false,
});

function sendResponse(id, result, error) {
  const payload = {
    jsonrpc: '2.0',
    id: id ?? null,
  };
  if (error) {
    payload.error = {
      code: error.code || -32603,
      message: error.message || String(error),
    };
  } else {
    payload.result = result;
  }
  process.stdout.write(JSON.stringify(payload) + '\n');
}

rl.on('line', async (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  let req;
  try {
    req = JSON.parse(trimmed);
  } catch {
    sendResponse(null, null, { code: -32700, message: 'Parse error' });
    return;
  }

  const { id, method, params } = req;

  try {
    switch (method) {
      case 'initialize': {
        sendResponse(id, {
          protocolVersion: '2024-11-05',
          capabilities: { tools: {} },
          serverInfo: { name: 'agentxflow-mcp-gateway', version: '0.1.0' },
        });
        break;
      }
      case 'notifications/initialized': {
        break;
      }
      case 'tools/list': {
        sendResponse(id, { tools: TOOLS });
        break;
      }
      case 'tools/call': {
        const { name, arguments: toolArgs } = params || {};
        try {
          const res = await handleToolCall(name, toolArgs || {});
          sendResponse(id, res);
        } catch (callErr) {
          sendResponse(id, {
            content: [
              {
                type: 'text',
                text: `AgentXFlow error: ${callErr.message}`,
              },
            ],
            isError: true,
          });
        }
        break;
      }
      case 'ping': {
        sendResponse(id, {});
        break;
      }
      default: {
        if (id !== undefined && id !== null) {
          sendResponse(id, null, { code: -32601, message: `Method not found: ${method}` });
        }
        break;
      }
    }
  } catch (err) {
    sendResponse(id, null, { code: -32603, message: err.message });
  }
});
