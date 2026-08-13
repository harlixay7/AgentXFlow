import React, { useState, useEffect } from 'react';
import { McpInfo, Agent } from '../types';
import { coordinatorApi } from '../api/coordinator';
import { Terminal, Copy, Check, ShieldCheck, Activity, Cpu, Server, FileText, Plus, Bot, Layers } from 'lucide-react';

interface IntegrationsViewProps {
  agents?: Agent[];
  onRefreshAgents?: () => void;
}

export const IntegrationsView: React.FC<IntegrationsViewProps> = ({ agents = [], onRefreshAgents }) => {
  const [mcpInfo, setMcpInfo] = useState<McpInfo | null>(null);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<'antigravity' | 'claude' | 'cursor' | 'opencode' | 'generic'>('antigravity');
  const [pingStatus, setPingStatus] = useState<'idle' | 'testing' | 'success' | 'error'>('idle');

  // Agent Fleet Registration State
  const [agentName, setAgentName] = useState('');
  const [agentType, setAgentType] = useState('Antigravity');
  const [isRegistering, setIsRegistering] = useState(false);

  useEffect(() => {
    coordinatorApi.getMcpInfo().then(setMcpInfo).catch(console.error);
  }, []);

  const copyText = (text: string, key: string) => {
    navigator.clipboard.writeText(text);
    setCopiedKey(key);
    setTimeout(() => setCopiedKey(null), 2000);
  };

  const handlePingTest = async () => {
    setPingStatus('testing');
    try {
      const res = await fetch('http://127.0.0.1:7890/health');
      if (res.ok) {
        setPingStatus('success');
      } else {
        setPingStatus('error');
      }
    } catch (e) {
      setPingStatus('error');
    }
  };

  const handleRegisterAgent = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!agentName.trim()) return;
    setIsRegistering(true);
    try {
      await coordinatorApi.registerAgent(agentName.trim(), agentType);
      setAgentName('');
      if (onRefreshAgents) onRefreshAgents();
      alert(`Successfully registered "${agentName.trim()}" (${agentType}) to your fleet!`);
    } catch (err: any) {
      alert(`Registration failed: ${err.message || err}`);
    } finally {
      setIsRegistering(false);
    }
  };

  const openCodeConfig = mcpInfo
    ? JSON.stringify(
        {
          mcpServers: {
            agentxflow: {
              url: mcpInfo.url,
              transport: 'http',
              headers: {
                Authorization: `Bearer ${mcpInfo.token}`,
              },
            },
          },
        },
        null,
        2
      )
    : '';

  const cursorMcpConfig = mcpInfo
    ? JSON.stringify(
        {
          mcpServers: {
            agentxflow: {
              command: 'node',
              args: ['scripts/test_mcp_workflow.js'],
              url: mcpInfo.url,
              headers: {
                Authorization: `Bearer ${mcpInfo.token}`,
              },
            },
          },
        },
        null,
        2
      )
    : '';

  return (
    <div style={{ flex: 1, padding: 24, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 20 }}>
      {/* View Header */}
      <div>
        <h2 style={{ fontSize: 16, fontWeight: 700, marginBottom: 4, display: 'flex', alignItems: 'center', gap: 8, fontFamily: 'var(--font-mono)' }}>
          <ShieldCheck size={18} style={{ color: 'var(--accent-blue)' }} />
          Localhost MCP Gateway & Multi-IDE Fleet Manager
        </h2>
        <p style={{ color: 'var(--text-secondary)', fontSize: 12, maxWidth: 850, lineHeight: 1.5 }}>
          AgentXFlow runs an authoritative Model Context Protocol (MCP) server on <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--accent-blue)' }}>127.0.0.1:7890</code>. <strong>There is no limit on connected agents or IDEs</strong>: you can connect 6+ IDEs simultaneously (Antigravity, Cursor, Claude Code, Codex, OpenCode, Gemini) with each agent running inside its own isolated Git worktree.
        </p>
      </div>

      {/* Server Status Card */}
      {mcpInfo && (
        <div style={{ padding: 16, backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-md)' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
            <div style={{ fontWeight: 600, fontSize: 12, display: 'flex', alignItems: 'center', gap: 6, fontFamily: 'var(--font-mono)' }}>
              <span style={{ width: 8, height: 8, borderRadius: '50%', backgroundColor: 'var(--accent-green)' }} />
              Gateway Status: ONLINE (127.0.0.1:7890)
            </div>
            <button
              className="btn btn-secondary"
              style={{ fontSize: 11, padding: '4px 10px' }}
              onClick={handlePingTest}
              title="Send an HTTP ping to http://127.0.0.1:7890/health to verify endpoint connectivity"
            >
              <Activity size={12} />
              {pingStatus === 'testing' ? 'Pinging...' : pingStatus === 'success' ? '200 OK (Gateway Responding)' : pingStatus === 'error' ? 'Connection Error' : 'Test Health Ping'}
            </button>
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 10, fontFamily: 'var(--font-mono)', fontSize: 11 }}>
            <div style={{ padding: 10, backgroundColor: 'var(--bg-input)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)' }}>
              <div style={{ color: 'var(--text-muted)', fontSize: 10, marginBottom: 2 }}>STREAMABLE HTTP ENDPOINT</div>
              <div style={{ color: 'var(--accent-blue)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span>{mcpInfo.url}</span>
                <button
                  style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}
                  onClick={() => copyText(mcpInfo.url, 'url')}
                  title="Copy MCP endpoint URL"
                >
                  {copiedKey === 'url' ? <Check size={12} style={{ color: 'var(--accent-green)' }} /> : <Copy size={12} />}
                </button>
              </div>
            </div>

            <div style={{ padding: 10, backgroundColor: 'var(--bg-input)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)' }}>
              <div style={{ color: 'var(--text-muted)', fontSize: 10, marginBottom: 2 }}>LEGACY SSE ENDPOINT</div>
              <div style={{ color: 'var(--accent-purple)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span>{mcpInfo.sse_url}</span>
                <button
                  style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}
                  onClick={() => copyText(mcpInfo.sse_url, 'sse')}
                  title="Copy legacy SSE URL"
                >
                  {copiedKey === 'sse' ? <Check size={12} style={{ color: 'var(--accent-green)' }} /> : <Copy size={12} />}
                </button>
              </div>
            </div>

            <div style={{ padding: 10, backgroundColor: 'var(--bg-input)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)' }}>
              <div style={{ color: 'var(--text-muted)', fontSize: 10, marginBottom: 2 }}>BEARER AUTH TOKEN</div>
              <div style={{ color: 'var(--text-primary)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span>{mcpInfo.token.substring(0, 16)}...</span>
                <button
                  style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}
                  onClick={() => copyText(mcpInfo.token, 'token')}
                  title="Copy authentication token"
                >
                  {copiedKey === 'token' ? <Check size={12} style={{ color: 'var(--accent-green)' }} /> : <Copy size={12} />}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Agent Fleet Manager Section (Connect unlimited IDEs) */}
      <div style={{ backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-md)', padding: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
          <div>
            <h3 style={{ fontSize: 13, fontWeight: 700, fontFamily: 'var(--font-mono)', display: 'flex', alignItems: 'center', gap: 6 }}>
              <Layers size={14} style={{ color: 'var(--accent-blue)' }} />
              Connected Agent & IDE Fleet ({agents.length} Registered)
            </h3>
            <p style={{ fontSize: 11, color: 'var(--text-secondary)', marginTop: 2 }}>
              Add any number of Antigravity, Cursor, Claude, or Codex instances to collaborate on tasks concurrently.
            </p>
          </div>
        </div>

        {/* Quick Add Form */}
        <form onSubmit={handleRegisterAgent} style={{ display: 'flex', gap: 8, marginBottom: 14, flexWrap: 'wrap' }}>
          <input
            type="text"
            className="input-field"
            placeholder="IDE / Agent Name (e.g. Antigravity-Main, Cursor-Frontend)"
            value={agentName}
            onChange={(e) => setAgentName(e.target.value)}
            style={{ flex: 1, minWidth: 220, height: 30, fontSize: 11 }}
            required
          />
          <select
            className="input-field"
            value={agentType}
            onChange={(e) => setAgentType(e.target.value)}
            style={{ width: 160, height: 30, fontSize: 11 }}
          >
            <option value="Antigravity">Antigravity</option>
            <option value="Cursor">Cursor IDE</option>
            <option value="Claude">Claude Code</option>
            <option value="Codex">Codex CLI</option>
            <option value="OpenCode">OpenCode</option>
            <option value="Gemini">Gemini CLI</option>
            <option value="Generic">Generic MCP Client</option>
          </select>
          <button
            type="submit"
            className="btn btn-primary"
            disabled={isRegistering || !agentName.trim()}
            style={{ height: 30, fontSize: 11, padding: '0 12px' }}
          >
            <Plus size={13} /> Register IDE
          </button>
        </form>

        {/* Fleet Grid */}
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))', gap: 10 }}>
          {agents.map((ag) => (
            <div
              key={ag.id}
              style={{
                padding: 10,
                backgroundColor: 'var(--bg-card)',
                border: '1px solid var(--border-subtle)',
                borderRadius: 'var(--radius-sm)',
                display: 'flex',
                alignItems: 'center',
                gap: 8,
              }}
            >
              <div
                style={{
                  width: 28,
                  height: 28,
                  borderRadius: 'var(--radius-sm)',
                  backgroundColor: ag.agent_type === 'Antigravity' ? 'rgba(88, 166, 255, 0.15)' : 'rgba(163, 113, 247, 0.15)',
                  color: ag.agent_type === 'Antigravity' ? 'var(--accent-blue)' : 'var(--accent-purple)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  flexShrink: 0,
                }}
              >
                <Bot size={15} />
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontWeight: 600, fontSize: 11, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {ag.name}
                </div>
                <div style={{ fontSize: 10, color: 'var(--text-muted)', display: 'flex', gap: 6 }}>
                  <span>{ag.agent_type}</span>
                  <span>•</span>
                  <span style={{ color: 'var(--accent-green)' }}>{ag.status}</span>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Client Configuration Setup Snippets */}
      <div style={{ backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-md)', overflow: 'hidden' }}>
        <div style={{ display: 'flex', borderBottom: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-input)' }}>
          <div
            className={`nav-item ${activeTab === 'antigravity' ? 'active' : ''}`}
            style={{ borderRadius: 0, padding: '10px 16px', fontSize: 11 }}
            onClick={() => setActiveTab('antigravity')}
            title="Configuration for Antigravity IDE instances"
          >
            <FileText size={13} /> Antigravity IDE
          </div>
          <div
            className={`nav-item ${activeTab === 'cursor' ? 'active' : ''}`}
            style={{ borderRadius: 0, padding: '10px 16px', fontSize: 11 }}
            onClick={() => setActiveTab('cursor')}
            title="Configuration for Cursor IDE"
          >
            <Cpu size={13} /> Cursor IDE
          </div>
          <div
            className={`nav-item ${activeTab === 'claude' ? 'active' : ''}`}
            style={{ borderRadius: 0, padding: '10px 16px', fontSize: 11 }}
            onClick={() => setActiveTab('claude')}
            title="Configuration for Claude Code / Codex"
          >
            <Terminal size={13} /> Claude Code / Codex
          </div>
          <div
            className={`nav-item ${activeTab === 'opencode' ? 'active' : ''}`}
            style={{ borderRadius: 0, padding: '10px 16px', fontSize: 11 }}
            onClick={() => setActiveTab('opencode')}
            title="Configuration for OpenCode CLI"
          >
            <Server size={13} /> OpenCode Config
          </div>
          <div
            className={`nav-item ${activeTab === 'generic' ? 'active' : ''}`}
            style={{ borderRadius: 0, padding: '10px 16px', fontSize: 11 }}
            onClick={() => setActiveTab('generic')}
            title="Generic HTTP JSON-RPC 2.0"
          >
            <Server size={13} /> Generic MCP
          </div>
        </div>

        <div style={{ padding: 16 }}>
          {activeTab === 'antigravity' && (
            <div>
              <h4 style={{ fontSize: 12, fontWeight: 600, marginBottom: 6, fontFamily: 'var(--font-mono)' }}>
                Antigravity IDE & Subagents Setup
              </h4>
              <p style={{ fontSize: 11, color: 'var(--text-secondary)', lineHeight: 1.6, marginBottom: 10 }}>
                Antigravity connects natively via the <code>agentxflow-coordinator</code> skill. You can run multiple Antigravity windows or subagents simultaneously. Each Antigravity instance discovers tasks via <code>task.list</code>, claims its own worktree via <code>task.claim</code>, and locks file patterns via <code>scope.acquire</code>.
              </p>
              <div style={{ padding: 10, backgroundColor: 'var(--bg-input)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 11, fontFamily: 'var(--font-mono)' }}>
                Installed Skill: <code>.agents/skills/agentxflow-coordinator/SKILL.md</code>
              </div>
            </div>
          )}

          {activeTab === 'cursor' && (
            <div>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
                <span style={{ fontSize: 12, fontWeight: 600, fontFamily: 'var(--font-mono)' }}>Cursor MCP Configuration</span>
                <button
                  className="btn btn-secondary"
                  style={{ padding: '4px 8px', fontSize: 11 }}
                  onClick={() => copyText(cursorMcpConfig, 'cursor_json')}
                  title="Copy JSON configuration block"
                >
                  {copiedKey === 'cursor_json' ? <Check size={12} style={{ color: 'var(--accent-green)' }} /> : <Copy size={12} />}
                  {copiedKey === 'cursor_json' ? 'Copied' : 'Copy JSON'}
                </button>
              </div>
              <pre
                style={{
                  backgroundColor: 'var(--bg-input)',
                  border: '1px solid var(--border-subtle)',
                  padding: 12,
                  borderRadius: 'var(--radius-sm)',
                  fontFamily: 'var(--font-mono)',
                  fontSize: 11,
                  color: 'var(--text-primary)',
                  overflowX: 'auto',
                }}
              >
                {cursorMcpConfig}
              </pre>
            </div>
          )}

          {activeTab === 'claude' && (
            <div>
              <h4 style={{ fontSize: 12, fontWeight: 600, marginBottom: 6, fontFamily: 'var(--font-mono)' }}>Claude Code & Codex Setup</h4>
              <ol style={{ fontSize: 11, color: 'var(--text-secondary)', paddingLeft: 16, lineHeight: 1.8 }}>
                <li>MCP Gateway URL: <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--accent-blue)' }}>{mcpInfo?.url}</code></li>
                <li>Authorization Header: <code style={{ fontFamily: 'var(--font-mono)' }}>Authorization: Bearer {mcpInfo?.token}</code></li>
                <li>Agents automatically discover tasks via <code style={{ fontFamily: 'var(--font-mono)' }}>task.list</code> and submit verified changes via <code style={{ fontFamily: 'var(--font-mono)' }}>task.submit</code>.</li>
              </ol>
            </div>
          )}

          {activeTab === 'opencode' && (
            <div>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
                <span style={{ fontSize: 12, fontWeight: 600, fontFamily: 'var(--font-mono)' }}>OpenCode Client Configuration (.mcp.json)</span>
                <button
                  className="btn btn-secondary"
                  style={{ padding: '4px 8px', fontSize: 11 }}
                  onClick={() => copyText(openCodeConfig, 'json')}
                  title="Copy complete JSON configuration block"
                >
                  {copiedKey === 'json' ? <Check size={12} style={{ color: 'var(--accent-green)' }} /> : <Copy size={12} />}
                  {copiedKey === 'json' ? 'Copied' : 'Copy JSON'}
                </button>
              </div>
              <pre
                style={{
                  backgroundColor: 'var(--bg-input)',
                  border: '1px solid var(--border-subtle)',
                  padding: 12,
                  borderRadius: 'var(--radius-sm)',
                  fontFamily: 'var(--font-mono)',
                  fontSize: 11,
                  color: 'var(--text-primary)',
                  overflowX: 'auto',
                }}
              >
                {openCodeConfig}
              </pre>
            </div>
          )}

          {activeTab === 'generic' && (
            <div>
              <h4 style={{ fontSize: 12, fontWeight: 600, marginBottom: 6, fontFamily: 'var(--font-mono)' }}>Generic MCP Protocol</h4>
              <p style={{ fontSize: 11, color: 'var(--text-secondary)', lineHeight: 1.5 }}>
                Connect any standard MCP client to <code style={{ fontFamily: 'var(--font-mono)' }}>{mcpInfo?.url}</code> using standard JSON-RPC 2.0 tool calls.
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
