import React, { useState, useEffect } from 'react';
import { Task, Agent, TaskDetails, VerificationResult } from '../types';
import { X, Play, Shield, Send, CheckCircle, AlertTriangle, Bot, ArrowRight, RefreshCw, CheckSquare, Terminal } from 'lucide-react';
import { coordinatorApi } from '../api/coordinator';

interface TaskWorkspaceProps {
  task: Task;
  agents: Agent[];
  onClose: () => void;
  onRefresh: () => void;
}

export const TaskWorkspace: React.FC<TaskWorkspaceProps> = ({
  task,
  agents,
  onClose,
  onRefresh,
}) => {
  const [activeTab, setActiveTab] = useState<'overview' | 'plan' | 'verification' | 'leases' | 'agent'>('overview');
  const [activeAgentId, setActiveAgentId] = useState<string>(task.assigned_agent_id || agents[0]?.id || '');
  const [scopePattern, setScopePattern] = useState('src/**');
  const [verificationResult, setVerificationResult] = useState<VerificationResult | null>(null);
  const [taskDetails, setTaskDetails] = useState<TaskDetails | null>(null);
  const [loading, setLoading] = useState(false);
  const [completingStepId, setCompletingStepId] = useState<string | null>(null);
  const [stepEvidenceInput, setStepEvidenceInput] = useState<string>('cargo test --exit-code 0');

  const fetchDetails = async () => {
    try {
      const details = await coordinatorApi.getTaskDetails(task.id);
      setTaskDetails(details);
      if (details.task.assigned_agent_id) {
        setActiveAgentId(details.task.assigned_agent_id);
      }
    } catch (e) {
      console.error('Failed to load task details:', e);
    }
  };

  useEffect(() => {
    fetchDetails();
  }, [task.id]);

  const assignedAgent = agents.find((a) => a.id === (taskDetails?.task.assigned_agent_id || task.assigned_agent_id));
  const currentTask = taskDetails?.task || task;
  const steps = taskDetails?.steps || [];
  const criteria = taskDetails?.criteria || [];
  const leases = taskDetails?.leases || [];
  const runs = taskDetails?.verification_runs || [];

  const handleClaim = async () => {
    if (!activeAgentId) return;
    setLoading(true);
    try {
      await coordinatorApi.claimTask(currentTask.id, activeAgentId);
      await fetchDetails();
      onRefresh();
    } catch (e: any) {
      alert(e.toString());
    } finally {
      setLoading(false);
    }
  };

  const handleSatisfyCriterion = async (criterionId: string) => {
    setLoading(true);
    try {
      await coordinatorApi.satisfyAcceptanceCriterion(currentTask.id, criterionId, 'Manual User Review Sign-off');
      await fetchDetails();
      onRefresh();
    } catch (e: any) {
      alert(e.toString());
    } finally {
      setLoading(false);
    }
  };

  const handleScope = async () => {
    const agentId = currentTask.assigned_agent_id || activeAgentId;
    if (!agentId) return;
    setLoading(true);
    try {
      await coordinatorApi.requestScope(currentTask.id, agentId, [scopePattern]);
      await fetchDetails();
      onRefresh();
    } catch (e: any) {
      alert(e.toString());
    } finally {
      setLoading(false);
    }
  };

  const handleCompleteStep = async (stepId: string) => {
    setLoading(true);
    try {
      const agentId = currentTask.assigned_agent_id || activeAgentId || undefined;
      await coordinatorApi.completeStep(stepId, JSON.stringify({ stdout: stepEvidenceInput, exit_code: 0 }), agentId);
      setCompletingStepId(null);
      await fetchDetails();
      onRefresh();
    } catch (e: any) {
      alert(e.toString());
    } finally {
      setLoading(false);
    }
  };

  const handleSubmit = async () => {
    const agentId = currentTask.assigned_agent_id || activeAgentId;
    if (!agentId) return;
    setLoading(true);
    try {
      const res = await coordinatorApi.submitTask(currentTask.id, agentId);
      setVerificationResult(res);
      await fetchDetails();
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
        width: 560,
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
            <span className={`badge badge-${currentTask.state}`} title={`Current State: ${currentTask.state}`}>{currentTask.state}</span>
            <span className="badge badge-MEDIUM" title={`Execution Substate: ${currentTask.substate}`}>{currentTask.substate}</span>
            <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 700, fontSize: 12, color: 'var(--accent-blue)' }}>{currentTask.id}</span>
          </div>
          <div style={{ fontWeight: 600, fontSize: 13, color: 'var(--text-primary)' }}>{currentTask.title}</div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <button
            onClick={fetchDetails}
            style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', padding: 4 }}
            title="Refresh Task State"
          >
            <RefreshCw size={14} />
          </button>
          <button
            onClick={onClose}
            style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', padding: 4 }}
            title="Close Task Workspace drawer"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      {/* Guided Next Step Progress Bar */}
      <div style={{ padding: '8px 16px', backgroundColor: 'var(--bg-card)', borderBottom: '1px solid var(--border-subtle)', display: 'flex', alignItems: 'center', justifyContent: 'space-between', fontSize: 11 }}>
        <span style={{ color: 'var(--text-muted)' }}>Workflow Progression:</span>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontFamily: 'var(--font-mono)', fontSize: 10 }}>
          <span style={{ color: currentTask.assigned_agent_id ? 'var(--accent-green)' : 'var(--accent-yellow)', fontWeight: 600 }}>1. Claim</span>
          <ArrowRight size={10} style={{ color: 'var(--text-dim)' }} />
          <span style={{ color: currentTask.worktree_path ? 'var(--accent-green)' : 'var(--text-muted)', fontWeight: 600 }}>2. Worktree</span>
          <ArrowRight size={10} style={{ color: 'var(--text-dim)' }} />
          <span style={{ color: currentTask.state === 'REVIEW' || currentTask.state === 'DONE' ? 'var(--accent-green)' : 'var(--text-muted)', fontWeight: 600 }}>3. Verify</span>
          <ArrowRight size={10} style={{ color: 'var(--text-dim)' }} />
          <span style={{ color: currentTask.state === 'DONE' ? 'var(--accent-green)' : 'var(--text-muted)', fontWeight: 600 }}>4. Merge</span>
        </div>
      </div>

      {/* Tabs */}
      <div style={{ display: 'flex', borderBottom: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-input)' }}>
        {(['overview', 'plan', 'verification', 'leases', 'agent'] as const).map((tab) => (
          <div
            key={tab}
            className={`nav-item ${activeTab === tab ? 'active' : ''}`}
            style={{ borderRadius: 0, padding: '8px 14px', fontSize: 11, textTransform: 'capitalize' }}
            onClick={() => setActiveTab(tab)}
            title={`View ${tab} details and controls`}
          >
            {tab === 'plan' ? `Steps (${steps.length})` : tab === 'leases' ? `Scopes (${leases.length})` : tab}
          </div>
        ))}
      </div>

      {/* Tab Body */}
      <div style={{ flex: 1, padding: 16, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 16 }}>
        {activeTab === 'overview' && (
          <>
            <div>
              <div className="section-label" title="Instructions given to the AI agent">Task Description / Prompt</div>
              <div style={{ padding: 10, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 11, lineHeight: 1.5, whiteSpace: 'pre-wrap' }}>
                {currentTask.description}
              </div>
            </div>

            {/* Acceptance Criteria */}
            {criteria.length > 0 && (
              <div>
                <div className="section-label" title="Authoritative acceptance criteria that must pass">Acceptance Criteria ({criteria.length})</div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  {criteria.map((c) => (
                    <div key={c.id} style={{ padding: '8px 10px', backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', display: 'flex', alignItems: 'center', gap: 8, fontSize: 11 }}>
                      <CheckSquare size={13} style={{ color: c.is_satisfied ? 'var(--accent-green)' : 'var(--text-muted)' }} />
                      <span style={{ flex: 1, color: c.is_satisfied ? 'var(--text-primary)' : 'var(--text-secondary)' }}>{c.criterion}</span>
                      {c.is_satisfied ? (
                        <span className="badge badge-DONE">Satisfied</span>
                      ) : (
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                          <span className="badge badge-BACKLOG">Pending</span>
                          <button
                            className="btn btn-sm"
                            style={{ fontSize: 10, padding: '2px 8px', height: 'auto', backgroundColor: 'var(--bg-card)' }}
                            onClick={() => handleSatisfyCriterion(c.id)}
                            title="Sign-off on this criterion"
                          >
                            Sign-off
                          </button>
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            )}

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

            {currentTask.worktree_path && (
              <div>
                <div className="section-label" title="Isolated directory on disk where this agent writes code">Isolated Git Worktree Path</div>
                <div style={{ padding: 8, backgroundColor: 'var(--bg-input)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontFamily: 'var(--font-mono)', fontSize: 10, wordBreak: 'break-all', color: 'var(--accent-blue)' }}>
                  {currentTask.worktree_path}
                </div>
                <div style={{ fontSize: 10, color: 'var(--text-muted)', marginTop: 4 }}>
                  Branch: <code style={{ color: 'var(--accent-purple)' }}>{currentTask.branch_name}</code>
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
                  disabled={!currentTask.assigned_agent_id || loading}
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
              Required Execution Steps ({steps.length})
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {steps.length === 0 ? (
                <div style={{ padding: 12, textAlign: 'center', color: 'var(--text-muted)', fontSize: 11 }}>
                  No execution steps specified.
                </div>
              ) : (
                steps.map((s) => (
                  <div key={s.id} style={{ padding: 10, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', display: 'flex', flexDirection: 'column', gap: 6 }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                      <div style={{ fontWeight: 600, fontSize: 11 }}>{s.order_index}. {s.title}</div>
                      <span className={`badge ${s.status === 'COMPLETED' ? 'badge-DONE' : 'badge-BACKLOG'}`}>{s.status}</span>
                    </div>
                    {s.description && <div style={{ color: 'var(--text-muted)', fontSize: 10 }}>{s.description}</div>}

                    {s.status !== 'COMPLETED' && (
                      <div style={{ borderTop: '1px solid var(--border-subtle)', paddingTop: 6, marginTop: 4 }}>
                        {completingStepId === s.id ? (
                          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                            <input
                              className="input-field"
                              style={{ height: 24, fontSize: 10, fontFamily: 'var(--font-mono)' }}
                              value={stepEvidenceInput}
                              onChange={(e) => setStepEvidenceInput(e.target.value)}
                              placeholder="Test evidence or execution log"
                            />
                            <div style={{ display: 'flex', gap: 6 }}>
                              <button
                                className="btn btn-primary"
                                style={{ height: 22, fontSize: 10 }}
                                onClick={() => handleCompleteStep(s.id)}
                                disabled={loading}
                              >
                                Submit Step Evidence
                              </button>
                              <button
                                className="btn btn-secondary"
                                style={{ height: 22, fontSize: 10 }}
                                onClick={() => setCompletingStepId(null)}
                              >
                                Cancel
                              </button>
                            </div>
                          </div>
                        ) : (
                          <button
                            className="btn btn-secondary"
                            style={{ height: 22, fontSize: 10 }}
                            onClick={() => setCompletingStepId(s.id)}
                          >
                            <Terminal size={11} /> Mark Complete with Evidence
                          </button>
                        )}
                      </div>
                    )}
                  </div>
                ))
              )}
            </div>
          </div>
        )}

        {activeTab === 'verification' && (
          <div>
            <div className="section-label" title="Independently verify code changes before review and merge">
              Authoritative Machine Verification
            </div>
            <p style={{ fontSize: 11, color: 'var(--text-secondary)', marginBottom: 10, lineHeight: 1.5 }}>
              The coordinator executes verification profiles and machine evaluators directly in the isolated worktree. Criteria satisfaction is strictly derived from passing evaluator results.
            </p>

            <button
              className="btn btn-primary"
              style={{ width: '100%', marginBottom: 12, height: 32 }}
              onClick={handleSubmit}
              disabled={!currentTask.assigned_agent_id || loading}
              title="Run authoritative coordinator verification profile and machine evaluators"
            >
              <Send size={13} /> Run Verification Profile & Submit
            </button>

            {taskDetails?.active_attempt && (
              <div style={{ padding: 10, marginBottom: 12, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 11 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
                  <span style={{ fontWeight: 600 }}>Attempt #{taskDetails.active_attempt.attempt_number}</span>
                  <span className={`badge badge-${taskDetails.active_attempt.status}`}>{taskDetails.active_attempt.status}</span>
                </div>
                <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-muted)' }}>
                  Base: {taskDetails.active_attempt.base_sha?.slice(0, 8) || 'N/A'} | Head: {taskDetails.active_attempt.head_sha?.slice(0, 8) || 'Uncommitted'}
                </div>
              </div>
            )}

            {verificationResult && (
              <div style={{ padding: 12, marginBottom: 12, backgroundColor: verificationResult.is_valid ? 'rgba(63, 185, 80, 0.1)' : 'rgba(248, 81, 73, 0.1)', border: `1px solid ${verificationResult.is_valid ? 'var(--accent-green)' : 'var(--accent-red)'}`, borderRadius: 'var(--radius-sm)' }}>
                <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 6, display: 'flex', alignItems: 'center', gap: 6 }}>
                  {verificationResult.is_valid ? <CheckCircle size={14} style={{ color: 'var(--accent-green)' }} /> : <AlertTriangle size={14} style={{ color: 'var(--accent-red)' }} />}
                  {verificationResult.is_valid ? 'Automated Verification Passed — Ready for Merge' : 'Verification Checks Failed'}
                </div>
                {verificationResult.rejection_reasons.map((r, i) => (
                  <div key={i} style={{ fontSize: 10, color: 'var(--text-secondary)', marginTop: 2 }}>• {r}</div>
                ))}
              </div>
            )}

            {/* Evaluator Results */}
            {taskDetails?.evaluator_results && taskDetails.evaluator_results.length > 0 && (
              <div style={{ marginBottom: 12 }}>
                <div className="section-label">Machine Evaluator Checks ({taskDetails.evaluator_results.length})</div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  {taskDetails.evaluator_results.map((ev) => (
                    <div key={ev.id} style={{ padding: 8, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 10, fontFamily: 'var(--font-mono)' }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 2 }}>
                        <span style={{ fontWeight: 600, color: ev.passed ? 'var(--accent-green)' : 'var(--accent-red)' }}>{ev.evaluator_name} ({ev.evaluator_type})</span>
                        <span>Exit {ev.exit_code} ({ev.duration_ms}ms)</span>
                      </div>
                      <div style={{ color: 'var(--text-muted)' }}>SHA256: {ev.output_sha256.slice(0, 12)}... | {ev.commit_sha.slice(0, 8)}</div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Historical Verification Runs */}
            <div className="section-label" style={{ marginTop: 8 }}>Test Runs & Proofs ({runs.length})</div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              {runs.length === 0 ? (
                <div style={{ padding: 10, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', color: 'var(--text-muted)', fontSize: 10, textAlign: 'center' }}>
                  No coordinator test runs recorded yet.
                </div>
              ) : (
                runs.map((r) => (
                  <div key={r.id} style={{ padding: 8, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 10, fontFamily: 'var(--font-mono)' }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 2 }}>
                      <span style={{ fontWeight: 600, color: r.is_passed ? 'var(--accent-green)' : 'var(--accent-red)' }}>{r.check_name}</span>
                      <span>Exit {r.exit_code} ({r.duration_ms}ms)</span>
                    </div>
                    <div style={{ color: 'var(--text-muted)' }}>Commit: {r.commit_sha} {r.is_stale ? '(STALE)' : ''}</div>
                  </div>
                ))
              )}
            </div>
          </div>
        )}

        {activeTab === 'leases' && (
          <div>
            <div className="section-label">Active Scope Leases ({leases.length})</div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              {leases.length === 0 ? (
                <div style={{ padding: 12, textAlign: 'center', color: 'var(--text-muted)', fontSize: 11 }}>
                  No active scope locks held on this task.
                </div>
              ) : (
                leases.map((l) => (
                  <div key={l.id} style={{ padding: 10, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                    <div>
                      <div style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 11, color: 'var(--accent-blue)' }}>{l.pattern}</div>
                      <div style={{ color: 'var(--text-muted)', fontSize: 10 }}>Agent: {l.agent_id} • Type: {l.access_type}</div>
                    </div>
                    <span className="badge badge-RUNNING">Locked</span>
                  </div>
                ))
              )}
            </div>
          </div>
        )}

        {activeTab === 'agent' && (
          <div>
            <div className="section-label">Agent Session Diagnostics</div>
            <div style={{ padding: 12, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 11, display: 'flex', flexDirection: 'column', gap: 8 }}>
              <div>Assigned Agent: <strong>{currentTask.assigned_agent_id || 'None'}</strong></div>
              <div>Base Ref: <code>{currentTask.base_sha || 'HEAD~1'}</code></div>
              <div>Current Head: <code>{currentTask.head_sha || 'Uncommitted'}</code></div>
              <div>Protocol: <code>MCP 2024-11-05 / Streamable HTTP</code></div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
