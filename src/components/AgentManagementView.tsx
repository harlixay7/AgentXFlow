import React, { useState } from 'react';
import { Agent } from '../types';
import { Bot, Trash2, Plus, Plug } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';

interface AgentManagementViewProps {
  agents: Agent[];
  onRefresh: () => void;
}

export const AgentManagementView: React.FC<AgentManagementViewProps> = ({ agents, onRefresh }) => {
  const [showAddModal, setShowAddModal] = useState(false);
  const [agentName, setAgentName] = useState('');
  const [agentType, setAgentType] = useState('Antigravity');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);

  const handleRemoveAgent = async (agentId: string, name: string) => {
    if (!window.confirm(`Are you sure you want to remove agent "${name}"? This will release its active scope locks.`)) {
      return;
    }
    setDeletingId(agentId);
    try {
      await invoke('unregister_agent', { agentId });
      onRefresh();
    } catch (err) {
      console.error('Failed to unregister agent:', err);
      alert(`Error removing agent: ${err}`);
    } finally {
      setDeletingId(null);
    }
  };

  const handleAddAgent = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!agentName.trim()) return;

    setIsSubmitting(true);
    try {
      await invoke('register_agent', { name: agentName.trim(), agentType });
      setAgentName('');
      setShowAddModal(false);
      onRefresh();
    } catch (err) {
      console.error('Failed to register agent:', err);
      alert(`Error registering agent: ${err}`);
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div style={{ flex: 1, padding: 20, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 16 }}>
      {/* Header */}
      <div style={{ borderBottom: '1px solid var(--border-medium)', paddingBottom: 12, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <div>
          <h2 style={{ fontSize: 15, fontWeight: 700, fontFamily: 'var(--font-mono)' }}>Registered Agents & ACP Runtime</h2>
          <p style={{ color: 'var(--text-secondary)', fontSize: 11 }}>
            Manage active AI IDE workers, normalized tool capabilities, and permissions.
          </p>
        </div>
        <button
          className="btn btn-primary"
          style={{ height: 28, fontSize: 11 }}
          onClick={() => setShowAddModal(true)}
          title="Manually register an agent or configure an IDE worker session"
        >
          <Plus size={12} /> Register Agent
        </button>
      </div>

      {/* Empty State */}
      {agents.length === 0 ? (
        <div
          style={{
            padding: 40,
            textAlign: 'center',
            backgroundColor: 'var(--bg-surface)',
            border: '1px dashed var(--border-medium)',
            borderRadius: 'var(--radius-lg)',
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            gap: 12,
          }}
        >
          <div
            style={{
              width: 48,
              height: 48,
              borderRadius: 24,
              backgroundColor: 'rgba(88, 166, 255, 0.1)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              color: 'var(--accent-blue)',
            }}
          >
            <Plug size={24} />
          </div>
          <div>
            <h3 style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-primary)', margin: 0 }}>No Agents Connected Yet</h3>
            <p style={{ fontSize: 11, color: 'var(--text-muted)', maxWidth: 460, marginTop: 4 }}>
              Agents connect automatically via the local MCP server on <code>http://127.0.0.1:7890/mcp</code>, or you can register one manually.
            </p>
          </div>
          <button
            className="btn btn-primary"
            style={{ fontSize: 11, padding: '6px 14px' }}
            onClick={() => setShowAddModal(true)}
          >
            <Plus size={12} /> Register Agent Manually
          </button>
        </div>
      ) : (
        /* Agent Cards Grid */
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))', gap: 12 }}>
          {agents.map((a) => {
            const isWorking = a.status === 'WORKING';
            const isDisconnected = a.status === 'DISCONNECTED';
            const isIdle = a.status === 'IDLE';

            const formatLastSeen = () => {
              if (a.last_seen_seconds === undefined || a.last_seen_seconds === null) return 'Active';
              if (a.last_seen_seconds < 10) return 'Active just now';
              if (a.last_seen_seconds < 60) return `Active ${a.last_seen_seconds}s ago`;
              if (a.last_seen_seconds < 3600) return `Seen ${Math.floor(a.last_seen_seconds / 60)}m ago`;
              return `Seen ${Math.floor(a.last_seen_seconds / 3600)}h ago`;
            };

            return (
              <div
                key={a.id}
                style={{
                  backgroundColor: 'var(--bg-surface)',
                  border: isWorking
                    ? '1px solid rgba(88, 166, 255, 0.4)'
                    : isDisconnected
                    ? '1px solid rgba(240, 140, 0, 0.3)'
                    : '1px solid var(--border-medium)',
                  borderRadius: 'var(--radius-md)',
                  padding: 14,
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 10,
                  position: 'relative',
                  boxShadow: isWorking ? '0 0 12px rgba(88, 166, 255, 0.1)' : 'none',
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <div
                      style={{
                        width: 28,
                        height: 28,
                        borderRadius: 6,
                        backgroundColor: isWorking
                          ? 'rgba(88, 166, 255, 0.15)'
                          : isDisconnected
                          ? 'rgba(139, 148, 158, 0.15)'
                          : 'rgba(63, 185, 80, 0.15)',
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        color: isWorking
                          ? 'var(--accent-blue)'
                          : isDisconnected
                          ? 'var(--text-muted)'
                          : 'var(--accent-green)',
                      }}
                    >
                      <Bot size={16} />
                    </div>
                    <div>
                      <div style={{ fontWeight: 700, fontSize: 12, display: 'flex', alignItems: 'center', gap: 6 }}>
                        {a.name}
                        <span
                          style={{
                            width: 6,
                            height: 6,
                            borderRadius: '50%',
                            backgroundColor: isWorking
                              ? 'var(--accent-blue)'
                              : isDisconnected
                              ? 'var(--text-muted)'
                              : 'var(--accent-green)',
                            display: 'inline-block',
                          }}
                        />
                      </div>
                      <div style={{ color: 'var(--text-muted)', fontSize: 10 }}>
                        {a.agent_type} · <span style={{ color: isDisconnected ? 'var(--accent-yellow)' : 'var(--text-secondary)' }}>{formatLastSeen()}</span>
                      </div>
                    </div>
                  </div>

                  <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                    <span
                      className={`badge ${
                        isWorking ? 'badge-RUNNING' : isDisconnected ? 'badge-CANCELLED' : 'badge-READY'
                      }`}
                      style={{ fontSize: 10 }}
                    >
                      {a.status}
                    </span>
                    <button
                      className="btn btn-secondary"
                      style={{
                        height: 24,
                        width: 24,
                        padding: 0,
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        color: 'var(--accent-red)',
                        borderColor: 'rgba(248, 81, 73, 0.3)',
                      }}
                      onClick={() => handleRemoveAgent(a.id, a.name)}
                      disabled={deletingId === a.id}
                      title="Remove this agent and release its session"
                    >
                      <Trash2 size={12} />
                    </button>
                  </div>
                </div>

                {/* Active in-flight Task Info */}
                {a.active_task_id && (
                  <div
                    style={{
                      padding: '8px 10px',
                      backgroundColor: 'rgba(88, 166, 255, 0.08)',
                      border: '1px solid rgba(88, 166, 255, 0.2)',
                      borderRadius: 'var(--radius-sm)',
                      fontSize: 11,
                      display: 'flex',
                      flexDirection: 'column',
                      gap: 4,
                    }}
                  >
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                      <span style={{ fontSize: 9, fontWeight: 700, color: 'var(--accent-blue)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
                        Current In-Flight Task
                      </span>
                      <span style={{ fontSize: 9, color: 'var(--text-muted)' }}>
                        {a.active_task_id.slice(0, 8)}...
                      </span>
                    </div>
                    <div style={{ fontWeight: 600, color: 'var(--text-primary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {a.active_task_title || a.active_task_id}
                    </div>
                  </div>
                )}

                <div style={{ padding: 8, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 10 }}>
                  <div style={{ color: 'var(--text-muted)', marginBottom: 4, fontWeight: 600 }}>ASSIGNED PROFILE</div>
                  <div style={{ color: 'var(--accent-blue)', fontWeight: 600 }}>{a.profile || 'Implementer'}</div>
                </div>

                {/* Interactive State Recovery Controls */}
                <div style={{ display: 'flex', gap: 6, paddingTop: 4, borderTop: '1px solid var(--border-subtle)' }}>
                  {a.active_task_id && (
                    <button
                      className="btn btn-secondary"
                      style={{
                        flex: 1,
                        height: 26,
                        fontSize: 10,
                        color: 'var(--accent-yellow)',
                        borderColor: 'rgba(240, 140, 0, 0.3)',
                      }}
                      onClick={async () => {
                        if (window.confirm(`Unclaim all in-flight tasks for ${a.name}? Steps will revert to PENDING and worktrees will be cleaned up.`)) {
                          try {
                            await invoke('unclaim_agent_tasks', { agentId: a.id });
                            onRefresh();
                          } catch (err) {
                            alert(`Failed to unclaim tasks: ${err}`);
                          }
                        }
                      }}
                      title="Revert all in-flight steps back to PENDING and clean up isolated worktree"
                    >
                      Unclaim Steps
                    </button>
                  )}
                  {(!isIdle || isDisconnected) && (
                    <button
                      className="btn btn-secondary"
                      style={{
                        flex: 1,
                        height: 26,
                        fontSize: 10,
                      }}
                      onClick={async () => {
                        try {
                          await invoke('force_agent_idle', { agentId: a.id });
                          onRefresh();
                        } catch (err) {
                          alert(`Failed to reset agent: ${err}`);
                        }
                      }}
                      title="Reset agent status to IDLE and unclaim any orphaned work"
                    >
                      Force Idle
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Manual Register Modal */}
      {showAddModal && (
        <div
          className="modal-backdrop"
          onClick={() => setShowAddModal(false)}
          style={{
            position: 'fixed',
            inset: 0,
            backgroundColor: 'rgba(1, 4, 9, 0.75)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            zIndex: 1100,
            padding: 20,
          }}
        >
          <div
            style={{
              width: '100%',
              maxWidth: 420,
              backgroundColor: 'var(--bg-surface)',
              border: '1px solid var(--border-bright)',
              borderRadius: 'var(--radius-lg)',
              padding: 20,
              display: 'flex',
              flexDirection: 'column',
              gap: 16,
              boxShadow: '0 20px 40px rgba(0, 0, 0, 0.8)',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <h3 style={{ margin: 0, fontSize: 14, fontWeight: 600 }}>Register Agent Instance</h3>
              <button
                className="btn btn-secondary"
                style={{ height: 24, width: 24, padding: 0 }}
                onClick={() => setShowAddModal(false)}
              >
                ✕
              </button>
            </div>

            <form onSubmit={handleAddAgent} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div>
                <label style={{ fontSize: 11, color: 'var(--text-secondary)', display: 'block', marginBottom: 4 }}>
                  Select Canonical AI IDE / Platform
                </label>
                <select
                  className="input-field"
                  style={{ width: '100%', height: 32, fontSize: 12 }}
                  value={agentName}
                  onChange={(e) => {
                    const val = e.target.value;
                    setAgentName(val);
                    if (val === 'Antigravity' || val === 'Cursor' || val === 'OpenCode' || val === 'GitHub Copilot' || val === 'Windsurf' || val === 'Junie') {
                      setAgentType('IDE');
                    } else {
                      setAgentType('CLI');
                    }
                  }}
                  required
                >
                  <option value="">-- Choose AI IDE / Worker Platform --</option>
                  <option value="Antigravity">Google Antigravity (IDE)</option>
                  <option value="Claude Code">Claude Code (CLI)</option>
                  <option value="Cursor">Cursor (IDE)</option>
                  <option value="OpenCode">OpenCode (IDE)</option>
                  <option value="OpenAI Codex">OpenAI Codex (CLI)</option>
                  <option value="Gemini CLI">Google Gemini CLI (CLI)</option>
                  <option value="GitHub Copilot">GitHub Copilot / VS Code (IDE)</option>
                  <option value="Windsurf">Codeium Windsurf (IDE)</option>
                  <option value="Junie">JetBrains Junie (IDE)</option>
                  <option value="Aider">Aider (CLI)</option>
                </select>
              </div>

              <div>
                <label style={{ fontSize: 11, color: 'var(--text-secondary)', display: 'block', marginBottom: 4 }}>
                  Agent Runtime Category
                </label>
                <select
                  className="input-field"
                  style={{ width: '100%', height: 32, fontSize: 12 }}
                  value={agentType}
                  onChange={(e) => setAgentType(e.target.value)}
                >
                  <option value="IDE">IDE (Integrated Desktop Assistant)</option>
                  <option value="CLI">CLI (Terminal Agent Engine)</option>
                  <option value="Autonomous Swarm">Autonomous Swarm Worker</option>
                  <option value="Reviewer">Reviewer / Gatekeeper</option>
                  <option value="Implementer">Implementer</option>
                </select>
              </div>

              <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 8 }}>
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setShowAddModal(false)}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="btn btn-primary"
                  disabled={isSubmitting || !agentName.trim()}
                >
                  {isSubmitting ? 'Registering...' : 'Register'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
};
