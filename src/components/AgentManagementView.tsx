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
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))', gap: 12 }}>
          {agents.map((a) => (
            <div
              key={a.id}
              style={{
                backgroundColor: 'var(--bg-surface)',
                border: '1px solid var(--border-medium)',
                borderRadius: 'var(--radius-md)',
                padding: 14,
                display: 'flex',
                flexDirection: 'column',
                gap: 10,
                position: 'relative',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <Bot size={16} style={{ color: 'var(--accent-blue)' }} />
                  <div>
                    <div style={{ fontWeight: 700, fontSize: 12 }}>{a.name}</div>
                    <div style={{ color: 'var(--text-muted)', fontSize: 10 }}>Type: {a.agent_type}</div>
                  </div>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <span className={`badge ${a.status === 'WORKING' ? 'badge-RUNNING' : 'badge-READY'}`}>{a.status}</span>
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
                    title="Remove this agent and release its scope locks"
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              </div>

              <div style={{ padding: 8, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 10 }}>
                <div style={{ color: 'var(--text-muted)', marginBottom: 4, fontWeight: 600 }}>ASSIGNED PROFILE</div>
                <div style={{ color: 'var(--accent-blue)', fontWeight: 600 }}>{a.profile || 'Implementer'}</div>
              </div>

              {/* Normalized Capability Set */}
              <div>
                <div style={{ color: 'var(--text-muted)', fontSize: 10, marginBottom: 4, fontWeight: 600 }}>CAPABILITY SET</div>
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, fontSize: 10 }}>
                  {Object.entries(a.capabilities || { read_files: true, write_files: true, terminal: true, streaming: true, steering: true, mcp: true }).map(([k, v]) => (
                    <span
                      key={k}
                      style={{
                        padding: '2px 5px',
                        backgroundColor: 'var(--bg-input)',
                        border: '1px solid var(--border-subtle)',
                        borderRadius: 3,
                        color: v ? 'var(--text-primary)' : 'var(--text-muted)',
                      }}
                    >
                      {k}
                    </span>
                  ))}
                </div>
              </div>
            </div>
          ))}
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
                  Agent / IDE Instance Name
                </label>
                <input
                  type="text"
                  className="input-field"
                  style={{ width: '100%', height: 32, fontSize: 12 }}
                  placeholder="e.g. Antigravity Lead, Claude Backend, Cursor UI"
                  value={agentName}
                  onChange={(e) => setAgentName(e.target.value)}
                  autoFocus
                  required
                />
              </div>

              <div>
                <label style={{ fontSize: 11, color: 'var(--text-secondary)', display: 'block', marginBottom: 4 }}>
                  IDE / Tool Type
                </label>
                <select
                  className="input-field"
                  style={{ width: '100%', height: 32, fontSize: 12 }}
                  value={agentType}
                  onChange={(e) => setAgentType(e.target.value)}
                >
                  <option value="Antigravity">Antigravity</option>
                  <option value="Claude">Claude Code</option>
                  <option value="Cursor">Cursor</option>
                  <option value="Codex">Codex CLI</option>
                  <option value="OpenCode">OpenCode</option>
                  <option value="Gemini">Gemini</option>
                  <option value="Custom">Custom Agent</option>
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
