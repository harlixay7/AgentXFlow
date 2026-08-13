import React, { useState } from 'react';
import { Task, Agent, TaskStep, AcceptanceCriteria, ScopeLease, VerificationResult } from '../types';
import { X, Play, Shield, Send, CheckCircle, AlertTriangle, Bot, ArrowRight } from 'lucide-react';
import { coordinatorApi } from '../api/coordinator';

interface TaskWorkspaceProps {
  task: Task;
  agents: Agent[];
  steps: TaskStep[];
  criteria: AcceptanceCriteria[];
  leases: ScopeLease[];
  onClose: () => void;
  onRefresh: () => void;
}

export const TaskWorkspace: React.FC<TaskWorkspaceProps> = ({
  task,
  agents,
  steps,
  onClose,
  onRefresh,
}) => {
  const [activeTab, setActiveTab] = useState<'overview' | 'plan' | 'verification' | 'agent'>('overview');
  const [activeAgentId, setActiveAgentId] = useState<string>(agents[0]?.id || '');
  const [scopePattern, setScopePattern] = useState('src/**');
  const [verificationResult, setVerificationResult] = useState<VerificationResult | null>(null);
  const [loading, setLoading] = useState(false);

  const assignedAgent = agents.find((a) => a.id === task.assigned_agent_id);

  const handleClaim = async () => {
    if (!activeAgentId) return;
    setLoading(true);
    try {
      await coordinatorApi.claimTask(task.id, activeAgentId);
      onRefresh();
    } catch (e: any) {
      alert(e.toString());
    } finally {
      setLoading(false);
    }
  };

  const handleScope = async () => {
    if (!task.assigned_agent_id) return;
    setLoading(true);
    try {
      await coordinatorApi.requestScope(task.id, task.assigned_agent_id, [scopePattern]);
      alert(`Exclusive write lock acquired for pattern: "${scopePattern}"`);
      onRefresh();
    } catch (e: any) {
      alert(e.toString());
    } finally {
      setLoading(false);
    }
  };

  const handleSubmit = async () => {
    if (!task.assigned_agent_id) return;
    setLoading(true);
    try {
      const res = await coordinatorApi.submitTask(task.id, task.assigned_agent_id);
      setVerificationResult(res);
      if (res.is_valid) {
        alert('Verification checks passed! Task moved to REVIEW.');
      } else {
        alert(`Verification rejected:\n${res.rejection_reasons.join('\n')}`);
      }
      onRefresh();
    } catch (e: any) {
      alert(e.toString());
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      style={{
        width: 540,
        backgroundColor: 'var(--bg-surface)',
        borderLeft: '1px solid var(--border-medium)',
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        boxShadow: '-8px 0 24px rgba(0, 0, 0, 0.5)',
      }}
    >
      {/* Workspace Header */}
      <div style={{ padding: '14px 16px', borderBottom: '1px solid var(--border-medium)', display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', backgroundColor: 'var(--bg-input)' }}>
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
            <span className={`badge badge-${task.state}`} title={`Current State: ${task.state}`}>{task.state}</span>
            <span className="badge badge-MEDIUM" title={`Execution Substate: ${task.substate}`}>{task.substate}</span>
            <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 700, fontSize: 12, color: 'var(--accent-blue)' }}>{task.id}</span>
          </div>
          <div style={{ fontWeight: 600, fontSize: 13, color: 'var(--text-primary)' }}>{task.title}</div>
        </div>
        <button
          onClick={onClose}
          style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', padding: 4 }}
          title="Close Task Workspace drawer"
        >
          <X size={16} />
        </button>
      </div>

      {/* Guided Next Step Progress Bar */}
      <div style={{ padding: '8px 16px', backgroundColor: 'var(--bg-card)', borderBottom: '1px solid var(--border-subtle)', display: 'flex', alignItems: 'center', justifyContent: 'space-between', fontSize: 11 }}>
        <span style={{ color: 'var(--text-muted)' }}>Workflow Progression:</span>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontFamily: 'var(--font-mono)', fontSize: 10 }}>
          <span style={{ color: task.assigned_agent_id ? 'var(--accent-green)' : 'var(--accent-yellow)', fontWeight: 600 }}>1. Claim</span>
          <ArrowRight size={10} style={{ color: 'var(--text-dim)' }} />
          <span style={{ color: task.worktree_path ? 'var(--accent-green)' : 'var(--text-muted)', fontWeight: 600 }}>2. Worktree</span>
          <ArrowRight size={10} style={{ color: 'var(--text-dim)' }} />
          <span style={{ color: task.state === 'REVIEW' || task.state === 'DONE' ? 'var(--accent-green)' : 'var(--text-muted)', fontWeight: 600 }}>3. Verify</span>
          <ArrowRight size={10} style={{ color: 'var(--text-dim)' }} />
          <span style={{ color: task.state === 'DONE' ? 'var(--accent-green)' : 'var(--text-muted)', fontWeight: 600 }}>4. Merge</span>
        </div>
      </div>

      {/* Tabs */}
      <div style={{ display: 'flex', borderBottom: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-input)' }}>
        {(['overview', 'plan', 'verification', 'agent'] as const).map((tab) => (
          <div
            key={tab}
            className={`nav-item ${activeTab === tab ? 'active' : ''}`}
            style={{ borderRadius: 0, padding: '8px 14px', fontSize: 11, textTransform: 'capitalize' }}
            onClick={() => setActiveTab(tab)}
            title={`View ${tab} details and controls`}
          >
            {tab}
          </div>
        ))}
      </div>

      {/* Tab Body */}
      <div style={{ flex: 1, padding: 16, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 16 }}>
        {activeTab === 'overview' && (
          <>
            <div>
              <div className="section-label" title="Instructions given to the AI agent">Task Description / Prompt</div>
              <div style={{ padding: 10, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 11, lineHeight: 1.5 }}>
                {task.description}
              </div>
            </div>

            {/* Agent Assignment & Worktree Allocation */}
            <div>
              <div className="section-label" title="Which AI agent is assigned to work on this task">Agent Assignment & Worktree</div>
              {assignedAgent ? (
                <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: 10, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)' }}>
                  <Bot size={18} style={{ color: 'var(--accent-blue)' }} />
                  <div style={{ flex: 1 }}>
                    <div style={{ fontWeight: 600, fontSize: 11 }}>{assignedAgent.name} ({assignedAgent.agent_type})</div>
                    <div style={{ color: 'var(--text-muted)', fontSize: 10 }}>Profile: {assignedAgent.profile}</div>
                  </div>
                  <span className="badge badge-RUNNING">Active Lease</span>
                </div>
              ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 8, padding: 10, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)' }}>
                  <p style={{ fontSize: 11, color: 'var(--text-secondary)' }}>
                    Assign an AI agent to cut a dedicated Git worktree and begin implementation.
                  </p>
                  <div style={{ display: 'flex', gap: 6 }}>
                    <select
                      className="input-field"
                      style={{ height: 26, fontSize: 11 }}
                      value={activeAgentId}
                      onChange={(e) => setActiveAgentId(e.target.value)}
                      title="Select an AI agent from your registered agent list"
                    >
                      {agents.map((a) => (
                        <option key={a.id} value={a.id}>
                          {a.name} ({a.agent_type} - {a.profile})
                        </option>
                      ))}
                    </select>
                    <button
                      className="btn btn-primary"
                      style={{ height: 26 }}
                      onClick={handleClaim}
                      disabled={loading}
                      title="Claim task: creates branch and worktree directory"
                    >
                      <Play size={12} /> Claim Task
                    </button>
                  </div>
                </div>
              )}
            </div>

            {task.worktree_path && (
              <div>
                <div className="section-label" title="Isolated directory on disk where this agent writes code">Isolated Git Worktree Path</div>
                <div style={{ padding: 8, backgroundColor: 'var(--bg-input)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontFamily: 'var(--font-mono)', fontSize: 10, wordBreak: 'break-all', color: 'var(--accent-blue)' }}>
                  {task.worktree_path}
                </div>
                <div style={{ fontSize: 10, color: 'var(--text-muted)', marginTop: 4 }}>
                  Branch: <code style={{ color: 'var(--accent-purple)' }}>{task.branch_name}</code>
                </div>
              </div>
            )}

            {/* Scope Reservation */}
            <div>
              <div className="section-label" title="Prevent conflicting file edits between multiple agents">Exclusive File Write Lock</div>
              <div style={{ display: 'flex', gap: 6 }}>
                <input
                  className="input-field"
                  style={{ height: 26, fontSize: 11 }}
                  value={scopePattern}
                  onChange={(e) => setScopePattern(e.target.value)}
                  placeholder="e.g. src/auth/**"
                  title="Glob pattern for files this task is authorized to modify"
                />
                <button
                  className="btn btn-secondary"
                  style={{ height: 26 }}
                  onClick={handleScope}
                  disabled={!task.assigned_agent_id || loading}
                  title="Acquire exclusive write lock so other agents cannot edit matching files"
                >
                  <Shield size={12} /> Lock Scope
                </button>
              </div>
            </div>
          </>
        )}

        {activeTab === 'plan' && (
          <div>
            <div className="section-label" title="Mandatory steps that must be marked COMPLETED before task submission">
              Required Execution Steps
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {steps.map((s) => (
                <div key={s.id} style={{ padding: 10, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                  <div>
                    <div style={{ fontWeight: 600, fontSize: 11 }}>{s.order_index}. {s.title}</div>
                    {s.description && <div style={{ color: 'var(--text-muted)', fontSize: 10 }}>{s.description}</div>}
                  </div>
                  <span className={`badge ${s.status === 'COMPLETED' ? 'badge-DONE' : 'badge-BACKLOG'}`}>{s.status}</span>
                </div>
              ))}
            </div>
          </div>
        )}

        {activeTab === 'verification' && (
          <div>
            <div className="section-label" title="Independently verify code changes before review and merge">
              Authoritative Verification Gate
            </div>
            <p style={{ fontSize: 11, color: 'var(--text-secondary)', marginBottom: 10, lineHeight: 1.5 }}>
              The coordinator runs configured test scripts (e.g. <code>cargo test</code>, <code>npm test</code>) directly inside the task worktree.
            </p>

            <button
              className="btn btn-primary"
              style={{ width: '100%', marginBottom: 12, height: 32 }}
              onClick={handleSubmit}
              disabled={!task.assigned_agent_id || loading}
              title="Run authoritative coordinator checks and submit task for human review"
            >
              <Send size={13} /> Run Verification & Submit For Review
            </button>

            {verificationResult && (
              <div style={{ padding: 12, backgroundColor: verificationResult.is_valid ? 'rgba(63, 185, 80, 0.1)' : 'rgba(248, 81, 73, 0.1)', border: `1px solid ${verificationResult.is_valid ? 'var(--accent-green)' : 'var(--accent-red)'}`, borderRadius: 'var(--radius-sm)' }}>
                <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 6, display: 'flex', alignItems: 'center', gap: 6 }}>
                  {verificationResult.is_valid ? <CheckCircle size={14} style={{ color: 'var(--accent-green)' }} /> : <AlertTriangle size={14} style={{ color: 'var(--accent-red)' }} />}
                  {verificationResult.is_valid ? 'Verification Passed & Sealed' : 'Verification Checks Failed'}
                </div>
                {verificationResult.rejection_reasons.map((r, i) => (
                  <div key={i} style={{ fontSize: 10, color: 'var(--text-secondary)', marginTop: 2 }}>• {r}</div>
                ))}
              </div>
            )}
          </div>
        )}

        {activeTab === 'agent' && (
          <div>
            <div className="section-label">Agent Session Diagnostics</div>
            <div style={{ padding: 12, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 11, display: 'flex', flexDirection: 'column', gap: 8 }}>
              <div>Assigned: <strong>{task.assigned_agent_id || 'None'}</strong></div>
              <div>Base Ref: <code>{task.base_sha || 'HEAD~1'}</code></div>
              <div>Current Head: <code>{task.head_sha || 'Uncommitted'}</code></div>
              <div>Protocol: <code>MCP 2026-07-28 / Streamable HTTP</code></div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
