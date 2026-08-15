#!/usr/bin/env node
/**
 * AgentXFlow Native Node.js MCP Client Helper
 * High-performance, dependency-free direct HTTP bridge to AgentXFlow Coordinator (http://127.0.0.1:7890/mcp).
 * Uses Node.js native fetch and Buffer, avoiding shell-escaping and PowerShell transport overhead.
 */

import * as fs from 'node:fs';
import * as path from 'node:path';
import * as os from 'node:os';

const MCP_PORT = process.env.AGENTXFLOW_PORT || 7890;
const MCP_URL = `http://127.0.0.1:${MCP_PORT}/mcp`;

const ROAMING_DIR = process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming');
const APPDATA_DIR = fs.existsSync(path.join(ROAMING_DIR, 'AgentXFlow'))
  ? path.join(ROAMING_DIR, 'AgentXFlow')
  : path.join(ROAMING_DIR, 'agentxflow');

const TOKEN_PATH = path.join(APPDATA_DIR, '.agentxflow', 'auth.token');

export function getAuthToken() {
  if (process.env.AGENTXFLOW_TOKEN) {
    return process.env.AGENTXFLOW_TOKEN.trim();
  }
  try {
    if (fs.existsSync(TOKEN_PATH)) {
      return fs.readFileSync(TOKEN_PATH, 'utf8').trim();
    }
  } catch {}
  return '';
}

let cachedSessionToken = null;
let cachedAgentId = null;

export async function callTool(toolName, params = {}, sessionToken = null) {
  const token = sessionToken || cachedSessionToken || getAuthToken();
  const payload = {
    jsonrpc: '2.0',
    id: Date.now(),
    method: 'tools/call',
    params: {
      name: toolName,
      arguments: params || {},
    },
  };

  const bodyBuffer = Buffer.from(JSON.stringify(payload), 'utf8');

  const res = await fetch(MCP_URL, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': token ? `Bearer ${token}` : '',
      'MCP-Protocol-Version': '2024-11-05',
    },
    body: bodyBuffer,
  });

  if (!res.ok) {
    const errText = await res.text();
    throw new Error(`AgentXFlow HTTP ${res.status}: ${errText}`);
  }

  const json = await res.json();
  if (json.error) {
    throw new Error(`[${json.error.code}] ${json.error.message}`);
  }

  // Parse result content
  if (json.result && Array.isArray(json.result.content)) {
    const textItem = json.result.content.find((c) => c.type === 'text');
    if (textItem && typeof textItem.text === 'string') {
      try {
        return JSON.parse(textItem.text);
      } catch {
        return textItem.text;
      }
    }
  }

  return json.result;
}

export async function ensureAgentRegistered(agentName = 'Antigravity') {
  if (cachedSessionToken && cachedAgentId) {
    return { agentId: cachedAgentId, sessionToken: cachedSessionToken };
  }
  const regResult = await callTool('agent_register', { name: agentName });
  cachedAgentId = regResult.id || regResult.agent_id || agentName.toLowerCase();
  cachedSessionToken = regResult.session_token || getAuthToken();
  return { agentId: cachedAgentId, sessionToken: cachedSessionToken };
}

// CLI Execution Handler
async function main() {
  const args = process.argv.slice(2);
  const command = args[0];

  if (!command || command === '--help' || command === '-h') {
    console.log(`
AgentXFlow Native MCP Client CLI
Usage:
  node scripts/agentxflow_client.mjs context
  node scripts/agentxflow_client.mjs projects
  node scripts/agentxflow_client.mjs masterplan <project_id>
  node scripts/agentxflow_client.mjs decompose <project_id> <steps_file.json | inline_json>
  node scripts/agentxflow_client.mjs claim <project_id> [count] [agent_name]
  node scripts/agentxflow_client.mjs submit <task_id> [agent_id]
  node scripts/agentxflow_client.mjs call <tool_name> [json_params]
`);
    process.exit(0);
  }

  try {
    switch (command) {
      case 'context': {
        const res = await callTool('agentxflow_current_context');
        console.log(JSON.stringify(res, null, 2));
        break;
      }

      case 'projects': {
        const res = await callTool('project_list');
        console.log(JSON.stringify(res, null, 2));
        break;
      }

      case 'masterplan': {
        const projectId = args[1];
        if (!projectId) throw new Error('Usage: masterplan <project_id>');
        const res = await callTool('masterplan_get', { project_id: projectId });
        console.log(JSON.stringify(res, null, 2));
        break;
      }

      case 'decompose': {
        const projectId = args[1];
        const stepsSource = args[2];
        if (!projectId || !stepsSource) {
          throw new Error('Usage: decompose <project_id> <steps_file.json | inline_json>');
        }
        let steps = [];
        if (fs.existsSync(stepsSource)) {
          const content = fs.readFileSync(stepsSource, 'utf8');
          steps = JSON.parse(content);
        } else {
          steps = JSON.parse(stepsSource);
        }
        const res = await callTool('masterplan_decompose', {
          project_id: projectId,
          steps: Array.isArray(steps) ? steps : steps.steps || [],
          compact: true,
        });
        console.log(JSON.stringify(res, null, 2));
        break;
      }

      case 'claim': {
        const projectId = args[1];
        const count = args[2] ? parseInt(args[2], 10) : undefined;
        const agentName = args[3] || 'Antigravity';
        if (!projectId) throw new Error('Usage: claim <project_id> [count] [agent_name]');
        const { agentId, sessionToken } = await ensureAgentRegistered(agentName);
        const res = await callTool('masterplan_claim_chunk', {
          project_id: projectId,
          agent_id: agentId,
          count: count,
        }, sessionToken);
        console.log(JSON.stringify(res, null, 2));
        break;
      }

      case 'submit': {
        const taskId = args[1];
        const agentName = args[2] || 'Antigravity';
        if (!taskId) throw new Error('Usage: submit <task_id> [agent_name]');
        const { agentId, sessionToken } = await ensureAgentRegistered(agentName);
        const res = await callTool('task_submit', {
          task_id: taskId,
          agent_id: agentId,
        }, sessionToken);
        console.log(JSON.stringify(res, null, 2));
        break;
      }

      case 'call': {
        const toolName = args[1];
        const rawParams = args[2] ? JSON.parse(args[2]) : {};
        if (!toolName) throw new Error('Usage: call <tool_name> [json_params]');
        const res = await callTool(toolName, rawParams);
        console.log(JSON.stringify(res, null, 2));
        break;
      }

      default:
        throw new Error(`Unknown command: '${command}'. Run with --help for available commands.`);
    }
  } catch (err) {
    console.error(JSON.stringify({ error: err.message }, null, 2));
    process.exit(1);
  }
}

if (process.argv[1] && process.argv[1].endsWith('agentxflow_client.mjs')) {
  main();
}
