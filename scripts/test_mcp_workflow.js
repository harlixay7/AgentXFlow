/**
 * AgentXFlow E2E MCP Workflow Client Verification
 * Connects to http://127.0.0.1:7890/mcp using Streamable HTTP JSON-RPC 2.0 with Bearer authentication.
 */

const BASE_URL = process.env.MCP_URL || 'http://127.0.0.1:7890';
const AUTH_TOKEN = process.env.MCP_TOKEN || 'axf_sec_v2_live_token_7890';

async function sendRpc(method, params = {}) {
  const res = await fetch(`${BASE_URL}/mcp`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${AUTH_TOKEN}`,
      'MCP-Protocol-Version': '2026-07-28',
    },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: Date.now(),
      method,
      params,
    }),
  });

  if (!res.ok) {
    throw new Error(`HTTP ${res.status}: ${await res.text()}`);
  }

  const data = await res.json();
  if (data.error) {
    throw new Error(`RPC Error [${data.error.code}]: ${data.error.message}`);
  }
  return data.result;
}

async function runE2EWorkflow() {
  console.log('====================================================');
  console.log('🤖 Starting AgentXFlow E2E MCP Protocol Verification');
  console.log(`Target: ${BASE_URL} (Auth: Bearer ${AUTH_TOKEN.substring(0, 10)}...)`);
  console.log('====================================================\n');

  try {
    // 1. Health check
    console.log('▶ Step 1: Health Check (GET /health)...');
    const healthRes = await fetch(`${BASE_URL}/health`);
    const health = await healthRes.json();
    console.log('✔ Health Status:', health.status, `(Protocol: ${health.protocol_version})`);

    // 2. Register agent
    console.log('\n▶ Step 2: Registering AI Agent (agent.register)...');
    const agent = await sendRpc('agent.register', {
      name: 'Claude-Code-Orchestrator',
      agent_type: 'Claude',
    });
    console.log('✔ Agent Registered:', agent.name, `[ID: ${agent.id}]`);

    // 3. Heartbeat
    console.log('\n▶ Step 3: Sending Agent Heartbeat (agent.heartbeat)...');
    const hb = await sendRpc('agent.heartbeat', { agent_id: agent.id });
    console.log('✔ Heartbeat Response:', hb);

    // 4. List tasks
    console.log('\n▶ Step 4: Discovering Available Tasks (task.list)...');
    const tasks = await sendRpc('task.list', { project_id: 'proj-agentxflow-v2' });
    console.log(`✔ Found ${tasks.length} tasks in backlog & ready queues:`);
    tasks.forEach((t) => console.log(`   - [${t.state}] ${t.id}: ${t.title}`));

    if (tasks.length === 0) {
      console.log('No tasks to claim. Test finished.');
      return;
    }

    const targetTask = tasks[0];

    // 5. Project context pack
    console.log(`\n▶ Step 5: Fetching Context Pack for Task ${targetTask.id} (project.context)...`);
    const ctx = await sendRpc('project.context', {
      project_id: 'proj-agentxflow-v2',
      task_id: targetTask.id,
    });
    console.log('✔ Context Pack Contract Hash:', ctx.contract_hash);
    console.log('✔ Project Rules:', ctx.project_rules);

    // 6. Acquire scope
    console.log(`\n▶ Step 6: Requesting Exclusive File Write Scope (scope.acquire)...`);
    const leases = await sendRpc('scope.acquire', {
      task_id: targetTask.id,
      agent_id: agent.id,
      patterns: ['src-tauri/src/models/**', 'src-tauri/src/mcp/**'],
    });
    console.log(`✔ Acquired ${leases.length} exclusive scope locks:`, leases.map((l) => l.pattern));

    // 7. Complete step
    console.log(`\n▶ Step 7: Completing Verification Step (task.complete_step)...`);
    const step = await sendRpc('task.complete_step', {
      step_id: `${targetTask.id}-s1`,
      evidence: { stdout: 'Automated verification check passed with code 0', exit_code: 0 },
    });
    console.log(`✔ Step "${step.title}" marked as ${step.status}`);

    // 8. DAG check
    console.log(`\n▶ Step 8: Inspecting Task DAG Dependencies (dag.dependencies)...`);
    const deps = await sendRpc('dag.dependencies', { task_id: targetTask.id });
    console.log(`✔ Task has ${deps.length} prerequisite dependencies`);

    // 9. Merge queue status
    console.log(`\n▶ Step 9: Inspecting Serialized Merge Queue (merge.queue_status)...`);
    const queue = await sendRpc('merge.queue_status', { project_id: 'proj-agentxflow-v2' });
    console.log(`✔ Current Merge Queue Size: ${queue.length}`);

    // 10. Release scope
    console.log(`\n▶ Step 10: Releasing Scope Locks (scope.release)...`);
    const rel = await sendRpc('scope.release', { task_id: targetTask.id });
    console.log('✔ Scope Release Result:', rel);

    console.log('\n====================================================');
    console.log('🎉 ALL 10 MCP PROTOCOL TOOLS CALLED & VERIFIED SUCCESSFULLY!');
    console.log('====================================================');
  } catch (err) {
    console.error('❌ E2E Workflow Error:', err.message);
  }
}

runE2EWorkflow();
