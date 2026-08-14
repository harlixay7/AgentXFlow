import React, { useState, useEffect } from 'react';
import { Masterplan, MasterplanStep, Agent } from '../types';
import {
  Sparkles,
  Layers,
  CheckCircle2,
  Clock,
  Play,
  RotateCcw,
  Copy,
  Check,
  FileText,
  ShieldCheck,
  Cpu,
  FolderGit2,
  AlertCircle,
} from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';

interface MasterplanHubViewProps {
  projectId: string;
  agents: Agent[];
  onRefreshTasks: () => void;
}

export const MasterplanHubView: React.FC<MasterplanHubViewProps> = ({
  projectId,
  agents,
  onRefreshTasks,
}) => {
  const [masterplan, setMasterplan] = useState<Masterplan | null>(null);
  const [steps, setSteps] = useState<MasterplanStep[]>([]);
  const [loading, setLoading] = useState(true);
  const [rawText, setRawText] = useState('');
  const [targetStepCount, setTargetStepCount] = useState(20);
  const [maxStepsPerAgent, setMaxStepsPerAgent] = useState(4);
  const [isSaving, setIsSaving] = useState(false);
  const [isClaiming, setIsClaiming] = useState(false);
  const [copiedPrompt, setCopiedPrompt] = useState(false);
  const [filter, setFilter] = useState<'ALL' | 'PENDING' | 'CLAIMED' | 'COMPLETED'>('ALL');
  const [selectedAgentId, setSelectedAgentId] = useState<string>('');
  const [customChunkSize, setCustomChunkSize] = useState<number>(4);

  const fetchPlan = async () => {
    if (!projectId) return;
    try {
      const plan = await invoke<Masterplan | null>('get_masterplan', { projectId });
      setMasterplan(plan);
      if (plan) {
        setRawText(plan.raw_text);
        setTargetStepCount(plan.target_step_count);
        setMaxStepsPerAgent(plan.max_steps_per_agent);
        setCustomChunkSize(plan.max_steps_per_agent);
        const fetchedSteps = await invoke<MasterplanStep[]>('list_masterplan_steps', { projectId });
        setSteps(fetchedSteps);
      } else {
        setSteps([]);
      }
    } catch (err) {
      console.error('Failed to fetch masterplan:', err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchPlan();
  }, [projectId]);

  useEffect(() => {
    if (agents.length > 0 && !selectedAgentId) {
      setSelectedAgentId(agents[0].id);
    }
  }, [agents, selectedAgentId]);

  const handleSavePlan = async () => {
    if (!rawText.trim()) return;
    setIsSaving(true);
    try {
      const updated = await invoke<Masterplan>('create_or_update_masterplan', {
        projectId,
        rawText: rawText.trim(),
        targetStepCount: Number(targetStepCount),
        maxStepsPerAgent: Number(maxStepsPerAgent),
      });
      setMasterplan(updated);
      await fetchPlan();
      onRefreshTasks();
    } catch (err) {
      alert(`Error saving masterplan: ${err}`);
    } finally {
      setIsSaving(false);
    }
  };

  const handleResetPlan = async () => {
    if (!window.confirm('Reset this masterplan? All decomposed steps will be removed and the plan will return to UNSORTED.')) {
      return;
    }
    try {
      await invoke('reset_masterplan', { projectId });
      await fetchPlan();
      onRefreshTasks();
    } catch (err) {
      alert(`Error resetting plan: ${err}`);
    }
  };

  const handleClaimChunk = async () => {
    if (!selectedAgentId) {
      alert('Please select a registered AI agent to assign this chunk to.');
      return;
    }
    setIsClaiming(true);
    try {
      await invoke('claim_masterplan_chunk', {
        projectId,
        agentId: selectedAgentId,
        count: Number(customChunkSize),
      });
      await fetchPlan();
      onRefreshTasks();
    } catch (err) {
      alert(`Error claiming chunk: ${err}`);
    } finally {
      setIsClaiming(false);
    }
  };

  const handleParseStructuredSteps = async () => {
    if (!rawText.trim()) return;
    const lines = rawText.split('\n').map(l => l.trim()).filter(l => l.length > 0 && !l.startsWith('#'));
    if (lines.length === 0) return;

    const generatedSteps = lines.slice(0, targetStepCount).map((line, i) => {
      const idx = i + 1;
      const cleanTitle = line.replace(/^[0-9]+[.)\- ]+/, '').trim();
      return {
        step_index: idx,
        title: cleanTitle.slice(0, 80),
        description: `Execute specifications for: ${cleanTitle}`,
        suggested_scope: '',
        acceptance_criteria: `Automated test verification and criteria satisfied for Step #${idx}.`,
      };
    });

    try {
      await invoke('decompose_masterplan', {
        projectId,
        steps: generatedSteps,
      });
      await fetchPlan();
      onRefreshTasks();
    } catch (err) {
      alert(`Failed to decompose: ${err}`);
    }
  };

  const handleCopyMcpPrompt = () => {
    const prompt = `You are an autonomous engineering agent connected to AgentXFlow (by Viducia).
1. Call tool 'masterplan.get' with project_id '${projectId}'.
2. Read the masterplan text and decompose it into exactly ${targetStepCount} structured, non-overlapping sequential steps without missing any requirements.
3. Call 'masterplan.decompose' with your normalized steps array.
4. Call 'masterplan.claim_chunk' with your agent_id to claim the first ${maxStepsPerAgent} steps, execute them in your assigned Git worktree, and submit when verified.`;

    navigator.clipboard.writeText(prompt);
    setCopiedPrompt(true);
    setTimeout(() => setCopiedPrompt(false), 2500);
  };

  const totalSteps = steps.length;
  const pendingSteps = steps.filter((s) => s.status === 'PENDING').length;
  const claimedSteps = steps.filter((s) => s.status === 'CLAIMED' || s.status === 'IN_PROGRESS').length;
  const completedSteps = steps.filter((s) => s.status === 'COMPLETED').length;
  const progressPercent = totalSteps > 0 ? Math.round((completedSteps / totalSteps) * 100) : 0;

  const filteredSteps = steps.filter((s) => {
    if (filter === 'PENDING') return s.status === 'PENDING';
    if (filter === 'CLAIMED') return s.status === 'CLAIMED' || s.status === 'IN_PROGRESS';
    if (filter === 'COMPLETED') return s.status === 'COMPLETED';
    return true;
  });

  const getStatusBadge = (status: string) => {
    switch (status) {
      case 'UNSORTED':
        return <span className="badge" style={{ backgroundColor: 'rgba(210, 153, 34, 0.2)', color: 'var(--accent-yellow)', border: '1px solid rgba(210, 153, 34, 0.4)' }}>UNSORTED</span>;
      case 'RESORTED':
        return <span className="badge" style={{ backgroundColor: 'rgba(56, 139, 253, 0.2)', color: 'var(--accent-blue)', border: '1px solid rgba(56, 139, 253, 0.4)' }}>ORGANIZED & READY</span>;
      case 'EXECUTING':
        return <span className="badge badge-RUNNING">EXECUTING</span>;
      case 'COMPLETED':
        return <span className="badge badge-READY">COMPLETED</span>;
      default:
        return <span className="badge">{status}</span>;
    }
  };

  if (loading) {
    return (
      <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)' }}>
        Loading Masterplan Hub...
      </div>
    );
  }

  return (
    <div style={{ flex: 1, padding: 20, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 16 }}>
      {/* Header Banner */}
      <div
        style={{
          borderBottom: '1px solid var(--border-medium)',
          paddingBottom: 16,
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'flex-start',
          gap: 16,
          flexWrap: 'wrap',
        }}
      >
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
            <h2 style={{ fontSize: 16, fontWeight: 700, fontFamily: 'var(--font-mono)', margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
              <Sparkles size={16} style={{ color: 'var(--accent-blue)' }} /> Masterplan Execution Hub
            </h2>
            {masterplan && getStatusBadge(masterplan.status)}
          </div>
          <p style={{ color: 'var(--text-secondary)', fontSize: 11, margin: 0 }}>
            Paste any arbitrary master plan. The first AI agent autonomously normalizes requirements into structured steps, followed by sequential chunked execution.
          </p>
        </div>

        {/* Global Hub Action Controls */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <button
            className="btn btn-secondary"
            style={{ fontSize: 11, height: 28 }}
            onClick={handleCopyMcpPrompt}
            title="Copy autonomous prompt instructions to send to Claude Code, Antigravity, or Cursor"
          >
            {copiedPrompt ? <Check size={12} style={{ color: 'var(--accent-green)' }} /> : <Copy size={12} />}
            {copiedPrompt ? 'Copied Prompt' : 'Copy Agent Prompt'}
          </button>

          {masterplan && masterplan.status !== 'UNSORTED' && (
            <button
              className="btn btn-secondary"
              style={{ fontSize: 11, height: 28, color: 'var(--accent-red)' }}
              onClick={handleResetPlan}
              title="Reset plan back to UNSORTED raw text mode"
            >
              <RotateCcw size={12} /> Reset Plan
            </button>
          )}
        </div>
      </div>

      {/* Progress & Stats Bar (when organized) */}
      {masterplan && masterplan.status !== 'UNSORTED' && totalSteps > 0 && (
        <div
          style={{
            backgroundColor: 'var(--bg-surface)',
            border: '1px solid var(--border-medium)',
            borderRadius: 'var(--radius-md)',
            padding: 14,
            display: 'flex',
            flexDirection: 'column',
            gap: 10,
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <div style={{ fontSize: 12, fontWeight: 600 }}>
              Progress: <span style={{ color: 'var(--accent-blue)', fontFamily: 'var(--font-mono)' }}>{completedSteps} / {totalSteps}</span> Steps Merged ({progressPercent}%)
            </div>
            <div style={{ display: 'flex', gap: 12, fontSize: 11 }}>
              <span style={{ color: 'var(--text-secondary)' }}>
                <Clock size={11} style={{ display: 'inline', marginRight: 4 }} /> Pending: <b>{pendingSteps}</b>
              </span>
              <span style={{ color: 'var(--accent-yellow)' }}>
                <Layers size={11} style={{ display: 'inline', marginRight: 4 }} /> In Progress: <b>{claimedSteps}</b>
              </span>
              <span style={{ color: 'var(--accent-green)' }}>
                <CheckCircle2 size={11} style={{ display: 'inline', marginRight: 4 }} /> Completed: <b>{completedSteps}</b>
              </span>
            </div>
          </div>

          {/* Progress bar line */}
          <div style={{ width: '100%', height: 6, backgroundColor: 'var(--bg-input)', borderRadius: 3, overflow: 'hidden' }}>
            <div
              style={{
                width: `${progressPercent}%`,
                height: '100%',
                backgroundColor: progressPercent === 100 ? 'var(--accent-green)' : 'var(--accent-blue)',
                transition: 'width 0.3s ease',
              }}
            />
          </div>
        </div>
      )}

      {/* VIEW MODE 1: UNSORTED / RAW INPUT VIEW */}
      {(!masterplan || masterplan.status === 'UNSORTED') && (
        <div
          style={{
            backgroundColor: 'var(--bg-surface)',
            border: '1px solid var(--border-medium)',
            borderRadius: 'var(--radius-lg)',
            padding: 20,
            display: 'flex',
            flexDirection: 'column',
            gap: 16,
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <FileText size={16} style={{ color: 'var(--accent-blue)' }} />
              <h3 style={{ fontSize: 13, fontWeight: 700, margin: 0 }}>Input Raw Masterplan Specification</h3>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 12, fontSize: 11 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <label style={{ color: 'var(--text-secondary)' }} title="Target total step count the AI should decompose your plan into">
                  Target Steps:
                </label>
                <select
                  className="input-field"
                  style={{ height: 26, fontSize: 11, padding: '0 6px' }}
                  value={targetStepCount}
                  onChange={(e) => setTargetStepCount(Number(e.target.value))}
                >
                  <option value={10}>10 Steps</option>
                  <option value={20}>20 Steps</option>
                  <option value={30}>30 Steps</option>
                  <option value={50}>50 Steps</option>
                </select>
              </div>

              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <label style={{ color: 'var(--text-secondary)' }} title="Anti-hoarding cap: max sequential steps any single agent can claim at once">
                  Max Steps / Agent:
                </label>
                <select
                  className="input-field"
                  style={{ height: 26, fontSize: 11, padding: '0 6px' }}
                  value={maxStepsPerAgent}
                  onChange={(e) => setMaxStepsPerAgent(Number(e.target.value))}
                >
                  <option value={2}>2 Steps</option>
                  <option value={3}>3 Steps</option>
                  <option value={4}>4 Steps (Default)</option>
                  <option value={5}>5 Steps</option>
                </select>
              </div>
            </div>
          </div>

          {/* Raw Text Editor */}
          <div style={{ position: 'relative' }}>
            <textarea
              className="input-field"
              style={{
                width: '100%',
                minHeight: 240,
                fontFamily: 'var(--font-mono)',
                fontSize: 12,
                lineHeight: 1.6,
                padding: 14,
                resize: 'vertical',
              }}
              placeholder={`Paste your master plan here in any format (paragraphs, bullet points, PRD, or free text)...

Example:
1. Setup SQLite database schemas for user auth, session tokens, and password hashing.
2. Implement REST API endpoints for login, signup, and logout.
3. Write automated unit tests for password hashing and token expiration.
4. Implement frontend login and registration modal with error validation.
5. Setup Twilio webhook parser for incoming SMS notifications.
...`}
              value={rawText}
              onChange={(e) => setRawText(e.target.value)}
            />
            <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 6, color: 'var(--text-muted)', fontSize: 10 }}>
              <span>{rawText.length} characters • {rawText.split(/\s+/).filter(Boolean).length} words</span>
              <span>Status: <b style={{ color: 'var(--accent-yellow)' }}>UNSORTED</b></span>
            </div>
          </div>

          {/* Actions */}
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderTop: '1px solid var(--border-subtle)', paddingTop: 14 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, color: 'var(--text-muted)' }}>
              <AlertCircle size={13} style={{ color: 'var(--accent-yellow)' }} />
              Agents will automatically read this text over MCP and organize it into clean steps.
            </div>

            <div style={{ display: 'flex', gap: 8 }}>
              <button
                className="btn btn-secondary"
                style={{ fontSize: 11, height: 30 }}
                onClick={handleParseStructuredSteps}
                disabled={!rawText.trim()}
                title="Parse masterplan lines into structured steps"
              >
                <Sparkles size={12} /> Parse Steps
              </button>

              <button
                className="btn btn-primary"
                style={{ fontSize: 11, height: 30 }}
                onClick={handleSavePlan}
                disabled={isSaving || !rawText.trim()}
                title="Save masterplan and mark as ready for agent decomposition"
              >
                {isSaving ? 'Saving...' : 'Save & Prepare for Agents'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* VIEW MODE 2: RESORTED & ORGANIZED VISUAL CHECKLIST VIEW */}
      {masterplan && masterplan.status !== 'UNSORTED' && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
          {/* Action & Filter Toolbar */}
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              backgroundColor: 'var(--bg-surface)',
              border: '1px solid var(--border-medium)',
              borderRadius: 'var(--radius-md)',
              padding: 10,
              flexWrap: 'wrap',
              gap: 10,
            }}
          >
            {/* Filter Pills */}
            <div style={{ display: 'flex', gap: 4 }}>
              {(['ALL', 'PENDING', 'CLAIMED', 'COMPLETED'] as const).map((f) => (
                <button
                  key={f}
                  className={`btn ${filter === f ? 'btn-primary' : 'btn-secondary'}`}
                  style={{ height: 26, fontSize: 10, padding: '0 8px' }}
                  onClick={() => setFilter(f)}
                >
                  {f === 'ALL' ? `All (${totalSteps})` : f === 'PENDING' ? `Pending (${pendingSteps})` : f === 'CLAIMED' ? `In Progress (${claimedSteps})` : `Completed (${completedSteps})`}
                </button>
              ))}
            </div>

            {/* Quick Chunk Claim Launcher */}
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 11 }}>
                <span style={{ color: 'var(--text-secondary)' }}>Agent:</span>
                <select
                  className="input-field"
                  style={{ height: 26, fontSize: 11, padding: '0 6px', maxWidth: 140 }}
                  value={selectedAgentId}
                  onChange={(e) => setSelectedAgentId(e.target.value)}
                  title="Select registered agent to claim the next chunk"
                >
                  {agents.length === 0 ? (
                    <option value="">No agents connected</option>
                  ) : (
                    agents.map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.name} ({a.agent_type})
                      </option>
                    ))
                  )}
                </select>
              </div>

              <div style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 11 }}>
                <span style={{ color: 'var(--text-secondary)' }}>Chunk:</span>
                <select
                  className="input-field"
                  style={{ height: 26, fontSize: 11, padding: '0 6px' }}
                  value={customChunkSize}
                  onChange={(e) => setCustomChunkSize(Number(e.target.value))}
                  title="Number of steps to allocate in this worktree batch"
                >
                  <option value={2}>2 Steps</option>
                  <option value={3}>3 Steps</option>
                  <option value={4}>4 Steps</option>
                  <option value={5}>5 Steps</option>
                </select>
              </div>

              <button
                className="btn btn-primary"
                style={{ height: 26, fontSize: 11 }}
                onClick={handleClaimChunk}
                disabled={isClaiming || pendingSteps === 0 || !selectedAgentId}
                title="Atomically claim the next batch of pending steps and cut a dedicated Git worktree"
              >
                <Play size={11} /> {isClaiming ? 'Claiming...' : `Claim Next ${customChunkSize} Steps`}
              </button>
            </div>
          </div>

          {/* Visual Step Checklist */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {filteredSteps.map((step) => {
              const isClaimed = step.status === 'CLAIMED' || step.status === 'IN_PROGRESS';
              const isDone = step.status === 'COMPLETED';
              const assignedAgent = agents.find((a) => a.id === step.claimed_agent_id);

              return (
                <div
                  key={step.id}
                  style={{
                    backgroundColor: isDone ? 'rgba(35, 134, 54, 0.06)' : isClaimed ? 'rgba(56, 139, 253, 0.05)' : 'var(--bg-surface)',
                    border: `1px solid ${isDone ? 'rgba(46, 160, 67, 0.4)' : isClaimed ? 'rgba(56, 139, 253, 0.4)' : 'var(--border-medium)'}`,
                    borderRadius: 'var(--radius-md)',
                    padding: '12px 16px',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 8,
                    transition: 'border-color 0.2s ease',
                  }}
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 12 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                      <span
                        style={{
                          fontFamily: 'var(--font-mono)',
                          fontSize: 11,
                          fontWeight: 700,
                          padding: '2px 6px',
                          borderRadius: 'var(--radius-sm)',
                          backgroundColor: isDone ? 'var(--accent-green)' : isClaimed ? 'var(--accent-blue)' : 'var(--bg-input)',
                          color: isDone || isClaimed ? '#ffffff' : 'var(--text-secondary)',
                        }}
                      >
                        #{String(step.step_index).padStart(2, '0')}
                      </span>
                      <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-primary)' }}>
                        {step.title}
                      </span>
                    </div>

                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      {step.status === 'COMPLETED' ? (
                        <span className="badge badge-READY" style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                          <CheckCircle2 size={10} /> MERGED
                        </span>
                      ) : isClaimed ? (
                        <span className="badge badge-RUNNING" style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                          <Layers size={10} /> WORKING
                        </span>
                      ) : (
                        <span className="badge" style={{ backgroundColor: 'var(--bg-input)', color: 'var(--text-muted)' }}>
                          PENDING
                        </span>
                      )}
                    </div>
                  </div>

                  {/* Description */}
                  <div style={{ fontSize: 11, color: 'var(--text-secondary)', lineHeight: 1.5, whiteSpace: 'pre-wrap', paddingLeft: 34 }}>
                    {step.description}
                  </div>

                  {/* Metadata Footer: Scope Pill, Criteria, & Assigned Agent */}
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', paddingLeft: 34, paddingTop: 6, borderTop: '1px solid var(--border-subtle)', flexWrap: 'wrap', gap: 8, fontSize: 10 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                      <span style={{ display: 'flex', alignItems: 'center', gap: 4, color: 'var(--text-muted)' }} title="Target file glob scope reserved for this step">
                        <FolderGit2 size={11} style={{ color: 'var(--accent-blue)' }} />
                        <code>{step.suggested_scope}</code>
                      </span>

                      <span style={{ display: 'flex', alignItems: 'center', gap: 4, color: 'var(--text-muted)' }} title="Acceptance test criteria required to pass verification">
                        <ShieldCheck size={11} style={{ color: 'var(--accent-green)' }} />
                        {step.acceptance_criteria}
                      </span>
                    </div>

                    {isClaimed && (
                      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                        <span style={{ display: 'flex', alignItems: 'center', gap: 4, color: 'var(--accent-yellow)', fontWeight: 600 }}>
                          <Cpu size={11} /> {assignedAgent?.name || 'Assigned Agent'}
                        </span>
                        {step.claimed_task_id && (
                          <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-muted)' }}>
                            [{step.claimed_task_id}]
                          </span>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
};
