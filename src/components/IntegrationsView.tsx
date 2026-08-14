import React, { useState, useEffect } from 'react';
import { McpInfo, Agent } from '../types';
import { coordinatorApi } from '../api/coordinator';
import { Terminal, Copy, Check, ShieldCheck, Activity, Cpu, Server, FileText, Plus, Bot, Layers, CheckCircle2, AlertCircle } from 'lucide-react';

interface IntegrationsViewProps {
  agents?: Agent[];
  onRefreshAgents?: () => void;
}

export const IntegrationsView: React.FC<IntegrationsViewProps> = ({ agents = [], onRefreshAgents }) => {
  const [mcpInfo, setMcpInfo] = useState<McpInfo | null>(null);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<'antigravity' | 'claude' | 'cursor' | 'opencode' | 'generic'>('antigravity');
  const [pingStatus, setPingStatus] = useState<'idle' | 'testing' | 'success' | 'error'>('idle');
  const [feedback, setFeedback] = useState<{ message: string; type: 'success' | 'error' } | null>(null);

  const [agentName, setAgentName] = useState('');
  const [agentType, setAgentType] = useState('Antigravity');
  const [isRegistering, setIsRegistering] = useState(false);

  useEffect(() => {
    coordinatorApi.getMcpInfo().then(setMcpInfo).catch(console.error);
    fetch('http://127.0.0.1:7890/health')
      .then((res) => {
        if (res.ok) setPingStatus('success');
        else setPingStatus('error');
      })
      .catch(() => setPingStatus('error'));
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
    } catch {
      setPingStatus('error');
    }
  };

  const handleRegisterAgent = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!agentName.trim()) return;
    setIsRegistering(true);
    setFeedback(null);
    try {
      const registered = await coordinatorApi.registerAgent(agentName.trim(), agentType);
      setAgentName('');
      if (onRefreshAgents) onRefreshAgents();
      setFeedback({
        message: `Registered "${registered.name}" (${registered.agent_type}). Session token generated.`,
        type: 'success',
      });
    } catch (err: any) {
      setFeedback({
        message: `Registration failed: ${err.message || err}`,
        type: 'error',
      });
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
          AgentXFlow runs an authoritative Model Context Protocol (MCP) server on <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--accent-blue)' }}>127.0.0.1:7890</code>. Connect multiple agents and IDEs (Antigravity, Cursor, Claude Code, Codex, OpenCode) with each agent running in an isolated Git worktree.
        </p>
      </div>

      {feedback && (
        <div
          style={{
            padding: '10px 14px',
            borderRadius: 'var(--radius-sm)',
            fontSize: 12,
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            backgroundColor: feedback.type === 'success' ? 'rgba(34, 197, 94, 0.1)' : 'rgba(239, 68, 68, 0.1)',
            border: `1px solid ${feedback.type === 'success' ? 'var(--accent-green)' : 'var(--accent-red)'}`,
            color: feedback.type === 'success' ? 'var(--accent-green)' : 'var(--accent-red)',
          }}
        >
          {feedback.type === 'success' ? <CheckCircle2 size={14} /> : <AlertCircle size={14} />}
          <span style={{ userSelect: 'text' }}>{feedback.message}</span>
        </div>
      )}

      {/* Server Status Card */}
      {mcpInfo && (
        <div style={{ padding: 16, backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-md)' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
            <div style={{ fontWeight: 600, fontSize: 12, display: 'flex', alignItems: 'center', gap: 6, fontFamily: 'var(--font-mono)' }}>
              <span
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: '50%',
                  backgroundColor: pingStatus === 'success' ? 'var(--accent-green)' : pingStatus === 'error' ? 'var(--accent-red)' : 'var(--text-muted)',
                }}
              />
              Gateway Status: {pingStatus === 'success' ? 'ONLINE (127.0.0.1:7890)' : pingStatus === 'error' ? 'OFFLINE / UNREACHABLE' : 'CHECKING...'}
            </div>
            <button
              className="btn btn-secondary"
              style={{ fontSize: 11, padding: '4px 10px' }}
              onClick={handlePingTest}
              title="Send an HTTP ping to http://127.0.0.1:7890/health to verify endpoint connectivity"
            >
              <Activity size={12} />
              {pingStatus === 'testing' ? 'Pinging...' : pingStatus === 'success' ? '200 OK (Healthy)' : pingStatus === 'error' ? 'Connection Error' : 'Test Health Ping'}
            </button>
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 10, fontFamily: 'var(--font-mono)', fontSize: 11 }}>
            <div style={{ padding: 10, backgroundColor: 'var(--bg-input)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)' }}>
              <div style={{ color: 'var(--text-muted)', fontSize: 10, marginBottom: 2 }}>STREAMABLE HTTP ENDPOINT</div>
              <div style={{ color: 'var(--accent-blue)', display: 'flex', justifyContent: 'space-between', alignItems: 'center', userSelect: 'text' }}>
                <span style={{ userSelect: 'text' }}>{mcpInfo.url}</span>
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
              <div style={{ color: 'var(--accent-purple)', display: 'flex', justifyContent: 'space-between', alignItems: 'center', userSelect: 'text' }}>
                <span style={{ userSelect: 'text' }}>{mcpInfo.sse_url}</span>
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
              <div style={{ color: 'var(--text-muted)', fontSize: 10, marginBottom: 2 }}>BEARER BOOTSTRAP TOKEN</div>
              <div style={{ color: 'var(--text-primary)', display: 'flex', justifyContent: 'space-between', alignItems: 'center', userSelect: 'text' }}>
                <span style={{ userSelect: 'text' }}>{mcpInfo.token ? mcpInfo.token.substring(0, 16) + '...' : 'SECURE'}</span>
                <button
                  style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}
                  onClick={() => copyText(mcpInfo.token, 'token')}
                  title="Copy bootstrap token"
                >
                  {copiedKey === 'token' ? <Check size={12} style={{ color: 'var(--accent-green)' }} /> : <Copy size={12} />}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Agent Fleet Manager Section */}
      <div style={{ backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-md)', padding: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
          <div>
            <h3 style={{ fontSize: 13, fontWeight: 700, fontFamily: 'var(--font-mono)', display: 'flex', alignItems: 'center', gap: 6 }}>
              <Layers size={14} style={{ color: 'var(--accent-blue)' }} />
              Registered Agents & IDE Profiles ({agents.length})
            </h3>
            <p style={{ fontSize: 11, color: 'var(--text-secondary)', marginTop: 2 }}>
              Register agent identities to participate in task claiming and isolated worktree coordination.
            </p>
          </div>
        </div>

        {/* Quick Add Form */}
        <form onSubmit={handleRegisterAgent} style={{ display: 'flex', gap: 8, marginBottom: 14, flexWrap: 'wrap' }}>
          <input
            type="text"
            className="input-field"
            placeholder="Agent / IDE Name (e.g. Antigravity-Core, Cursor-Frontend)"
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
            <Plus size={13} /> Register Agent
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
                <div style={{ fontWeight: 600, fontSize: 11, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', userSelect: 'text' }}>
                  {ag.name}
                </div>
                <div style={{ fontSize: 10, color: 'var(--text-muted)', display: 'flex', gap: 6, userSelect: 'text' }}>
                  <span>{ag.agent_type}</span>
                  <span>•</span>
                  <span style={{ color: ag.status === 'WORKING' ? 'var(--accent-green)' : 'var(--text-muted)' }}>{ag.status}</span>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>

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
                Antigravity connects via the <code>agentxflow-coordinator</code> skill. Each agent instance discovers context via <code>agentxflow_current_context</code>, claims its own worktree via <code>task_claim</code> or <code>masterplan_claim_chunk</code>, and locks file patterns via <code>scope_acquire</code>.
              </p>
              <div style={{ padding: 10, backgroundColor: 'var(--bg-input)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 11, fontFamily: 'var(--font-mono)', userSelect: 'text' }}>
                Installed Skill: <code>SKILL.md</code> (AgentXFlow Coordinator Skill)
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
                  userSelect: 'text',
                }}
              >
                {cursorMcpConfig}
              </pre>
            </div>
          )}

          {activeTab === 'claude' && (
            <div>
              <h4 style={{ fontSize: 12, fontWeight: 600, marginBottom: 6, fontFamily: 'var(--font-mono)' }}>Claude Code & Codex Setup</h4>
              <ol style={{ fontSize: 11, color: 'var(--text-secondary)', paddingLeft: 16, lineHeight: 1.8, userSelect: 'text' }}>
                <li>MCP Gateway URL: <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--accent-blue)', userSelect: 'text' }}>{mcpInfo?.url}</code></li>
                <li>Authorization Header: <code style={{ fontFamily: 'var(--font-mono)', userSelect: 'text' }}>Authorization: Bearer {mcpInfo?.token}</code></li>
                <li>Agents automatically discover tasks via <code style={{ fontFamily: 'var(--font-mono)' }}>agentxflow_current_context</code> / <code style={{ fontFamily: 'var(--font-mono)' }}>task_list</code> and submit verified changes via <code style={{ fontFamily: 'var(--font-mono)' }}>task_submit</code>.</li>
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
                  userSelect: 'text',
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
                Connect any standard MCP client to <code style={{ fontFamily: 'var(--font-mono)', userSelect: 'text' }}>{mcpInfo?.url}</code> using standard JSON-RPC 2.0 tool calls.
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
