import React, { useState } from 'react';
import { Task, Agent, VerificationResult } from '../types';
import { X, CheckCircle, Shield, AlertTriangle, Send, Bot, Play, Info } from 'lucide-react';
import { coordinatorApi } from '../api/coordinator';

interface TaskDetailPanelProps {
  task: Task;
  agents: Agent[];
  onClose: () => void;
  onRefresh: () => void;
}

export const TaskDetailPanel: React.FC<TaskDetailPanelProps> = ({
  task,
  agents,
  onClose,
  onRefresh,
}) => {
  const [activeAgentId, setActiveAgentId] = useState<string>(agents[0]?.id || '');
  const [scopePattern, setScopePattern] = useState('src/**');
  const [verificationRes, setVerificationRes] = useState<VerificationResult | null>(null);
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
      setVerificationRes(res);
      onRefresh();
    } catch (e: any) {
      alert(e.toString());
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="task-detail-panel">
      <div className="panel-header">
        <div>
          <span className={`priority-tag priority-${task.priority}`} style={{ marginRight: 8 }} title={`Priority: ${task.priority}`}>
            {task.priority}
          </span>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-muted)' }} title="Task State Machine Status">
            STATE: {task.state}
          </span>
        </div>
        <button
          onClick={onClose}
          style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}
          title="Close detail panel"
        >
          <X size={16} />
        </button>
      </div>

      <div className="panel-body">
        <div>
          <h2 style={{ fontSize: 16, fontWeight: 700, marginBottom: 6, color: 'var(--text-primary)' }}>{task.title}</h2>
          <p style={{ color: 'var(--text-secondary)', fontSize: 12, lineHeight: 1.6, backgroundColor: 'var(--bg-card)', padding: 12, borderRadius: 'var(--radius-md)', border: '1px solid var(--border-subtle)' }}>
            {task.description}
          </p>
        </div>

        {/* Worktree & Branch Info */}
        {task.worktree_path && (
          <div
            style={{
              padding: 12,
              backgroundColor: 'var(--bg-app)',
              border: '1px solid var(--border-subtle)',
              borderRadius: 'var(--radius-md)',
              fontSize: 11,
              fontFamily: 'var(--font-mono)',
            }}
            title="Physical isolated Git Worktree directory allocated exclusively for this task"
          >
            <div style={{ color: 'var(--text-muted)', marginBottom: 4, display: 'flex', alignItems: 'center', gap: 4 }}>
              <Info size={12} /> ISOLATED GIT WORKTREE DIRECTORY
            </div>
            <div style={{ color: 'var(--accent-primary)', wordBreak: 'break-all', fontWeight: 600 }}>{task.worktree_path}</div>
          </div>
        )}

        {/* Agent Claim & Control Section */}
        <div style={{ padding: 14, border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-md)', backgroundColor: 'var(--bg-card)' }}>
          <div className="section-label" title="Assigned AI Agent responsible for this task">Agent Binding</div>

          {assignedAgent ? (
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginTop: 6 }}>
              <Bot size={20} style={{ color: 'var(--accent-primary)' }} />
              <div>
                <div style={{ fontWeight: 700, fontSize: 13 }}>{assignedAgent.name}</div>
                <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                  Assigned Agent Type: {assignedAgent.agent_type}
                </div>
              </div>
            </div>
          ) : (
            <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
              <select
                className="input-field"
                value={activeAgentId}
                onChange={(e) => setActiveAgentId(e.target.value)}
                title="Select agent to claim task"
              >
                {agents.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name} ({a.agent_type})
                  </option>
                ))}
              </select>
              <button className="btn btn-primary" onClick={handleClaim} disabled={loading} title="Claim task and generate isolated Git worktree for this agent">
                <Play size={13} /> Claim Task
              </button>
            </div>
          )}
        </div>

        {/* Scope Reservation Request */}
        <div style={{ padding: 14, border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-md)', backgroundColor: 'var(--bg-card)' }}>
          <div className="section-label" title="Declare glob pattern scope locks to prevent file collisions">
            Exclusive Write Scope Lock
          </div>
          <div style={{ fontSize: 11, color: 'var(--text-muted)', marginBottom: 8 }}>
            Agents declare glob patterns (e.g. <code style={{ fontFamily: 'var(--font-mono)' }}>src/auth/**</code>) before editing files.
          </div>
          <div style={{ display: 'flex', gap: 8 }}>
            <input
              className="input-field"
              value={scopePattern}
              onChange={(e) => setScopePattern(e.target.value)}
              placeholder="e.g. src/auth/**"
              title="Enter file glob pattern to lock"
            />
            <button className="btn btn-secondary" onClick={handleScope} disabled={!task.assigned_agent_id || loading} title="Grant scope lease to assigned agent">
              <Shield size={13} style={{ color: 'var(--accent-primary)' }} /> Reserve Scope
            </button>
          </div>
        </div>

        {/* Submission & Verification Gate Result */}
        {verificationRes && (
          <div
            style={{
              padding: 14,
              borderRadius: 'var(--radius-md)',
              border: verificationRes.is_valid
                ? '1px solid var(--accent-success)'
                : '1px solid var(--accent-danger)',
              backgroundColor: verificationRes.is_valid
                ? 'rgba(16, 185, 129, 0.12)'
                : 'rgba(239, 68, 68, 0.12)',
            }}
          >
            <div style={{ fontWeight: 700, display: 'flex', alignItems: 'center', gap: 6, marginBottom: 6 }}>
              {verificationRes.is_valid ? (
                <>
                  <CheckCircle size={16} style={{ color: 'var(--accent-success)' }} /> Verification Passed — Transitioned to REVIEW
                </>
              ) : (
                <>
                  <AlertTriangle size={16} style={{ color: 'var(--accent-danger)' }} /> Submission Rejected by Coordinator
                </>
              )}
            </div>

            {verificationRes.rejection_reasons.map((r, i) => (
              <div key={i} style={{ fontSize: 11, color: 'var(--text-secondary)', marginTop: 3 }}>
                • {r}
              </div>
            ))}
          </div>
        )}

        <div style={{ marginTop: 'auto', paddingTop: 16 }}>
          <button
            className="btn btn-primary"
            style={{ width: '100%', justifyContent: 'center', padding: '11px', fontSize: 13 }}
            onClick={handleSubmit}
            disabled={!task.assigned_agent_id || loading}
            title="Runs server-side verification checks for missing steps, evidence completeness, and scope cleanliness before allowing transition to REVIEW"
          >
            <Send size={15} /> Submit Task for Coordinator Verification
          </button>
        </div>
      </div>
    </div>
  );
};
