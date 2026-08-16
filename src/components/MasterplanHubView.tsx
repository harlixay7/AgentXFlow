import React, { useState, useEffect, useRef } from 'react';
import { Masterplan, MasterplanStep, Agent, Project } from '../types';
import {
  Sparkles,
  Layers,
  Play,
  RotateCcw,
  Copy,
  Check,
  FileText,
  AlertCircle,
  Edit3,
  Bot,
  Plus,
  Trash2,
  ArrowLeft,
  Eye,
  EyeOff,
  Radio,
  AlertTriangle,
  Zap,
} from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { coordinatorApi } from '../api/coordinator';

interface MasterplanHubViewProps {
  project: Project | null;
  projectId: string;
  agents: Agent[];
  onRefreshTasks: () => void;
}

export const MasterplanHubView: React.FC<MasterplanHubViewProps> = ({
  project,
  projectId,
  agents,
  onRefreshTasks,
}) => {
  // Masterplans catalog state
  const [masterplans, setMasterplans] = useState<Masterplan[]>([]);
  const [activePlanId, setActivePlanId] = useState<string | null>(null);
  const [selectedPlan, setSelectedPlan] = useState<Masterplan | null>(null);
  const [steps, setSteps] = useState<MasterplanStep[]>([]);
  const [loading, setLoading] = useState(true);

  // New Masterplan Modal State
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [newTitle, setNewTitle] = useState('');
  const [newRawText, setNewRawText] = useState('');
  const [newTargetSteps, setNewTargetSteps] = useState(20);
  const [newMaxStepsPerAgent, setNewMaxStepsPerAgent] = useState(4);
  const [newActivate, setNewActivate] = useState(true);
  const [isCreating, setIsCreating] = useState(false);

  // Conflict Modal State
  const [conflictModal, setConflictModal] = useState<{
    show: boolean;
    targetPlan: Masterplan | null;
    currentActivePlan: Masterplan | null;
  }>({
    show: false,
    targetPlan: null,
    currentActivePlan: null,
  });

  // Detailed Plan Inspection State
  const [rawText, setRawText] = useState('');
  const [targetStepCount, setTargetStepCount] = useState(20);
  const [maxStepsPerAgent, setMaxStepsPerAgent] = useState(4);
  const [isSaving, setIsSaving] = useState(false);
  const [isClaiming, setIsClaiming] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [copiedPrompt, setCopiedPrompt] = useState(false);
  const [filter, setFilter] = useState<'ALL' | 'PENDING' | 'CLAIMED' | 'COMPLETED'>('ALL');
  const [selectedAgentId, setSelectedAgentId] = useState<string>('');
  const [customChunkSize, setCustomChunkSize] = useState<number>(4);

  const [requireMilestoneApproval, setRequireMilestoneApproval] = useState<boolean>(true);
  const [isUpdatingMode, setIsUpdatingMode] = useState<boolean>(false);

  const lastSeqRef = useRef<number>(0);

  // Fetch all masterplans for this project
  const fetchAllPlans = async () => {
    if (!projectId) return;
    try {
      const plans = await coordinatorApi.listMasterplansForProject(projectId);
      setMasterplans(plans);

      // If we are currently inspecting a plan, refresh its data
      if (activePlanId) {
        const current = plans.find((p) => p.id === activePlanId) || null;
        setSelectedPlan(current);
        if (current) {
          setRawText(current.raw_text);
          setTargetStepCount(current.target_step_count);
          setMaxStepsPerAgent(current.max_steps_per_agent);
          setCustomChunkSize(current.max_steps_per_agent);
          setRequireMilestoneApproval(current.require_milestone_approval ?? true);
          const fetchedSteps = await coordinatorApi.listMasterplanStepsByPlanId(current.id);
          setSteps(fetchedSteps);
        } else {
          setSteps([]);
        }
      }
    } catch (err) {
      console.error('Failed to fetch masterplans:', err);
    } finally {
      setLoading(false);
    }
  };

  // Inspect a specific plan
  const handleOpenPlan = async (plan: Masterplan) => {
    setActivePlanId(plan.id);
    setSelectedPlan(plan);
    setRawText(plan.raw_text);
    setTargetStepCount(plan.target_step_count);
    setMaxStepsPerAgent(plan.max_steps_per_agent);
    setCustomChunkSize(plan.max_steps_per_agent);
    setRequireMilestoneApproval(plan.require_milestone_approval ?? true);
    setIsEditing(false);
    try {
      const fetchedSteps = await coordinatorApi.listMasterplanStepsByPlanId(plan.id);
      setSteps(fetchedSteps);
    } catch (err) {
      console.error('Failed to fetch steps for plan:', err);
      setSteps([]);
    }
  };

  const handleBackToCatalog = () => {
    setActivePlanId(null);
    setSelectedPlan(null);
    setSteps([]);
    fetchAllPlans();
  };

  // Toggle active status for a plan
  const handleToggleActive = async (plan: Masterplan, force: boolean = false) => {
    if (!plan.is_active) {
      // Trying to activate. Check if another plan is already active
      const currentlyActive = masterplans.find((p) => p.is_active && p.id !== plan.id);
      if (currentlyActive && !force) {
        // Show conflict modal
        setConflictModal({
          show: true,
          targetPlan: plan,
          currentActivePlan: currentlyActive,
        });
        return;
      }
    }

    try {
      await coordinatorApi.setMasterplanActiveToggle(plan.id, !plan.is_active, force);
      setConflictModal({ show: false, targetPlan: null, currentActivePlan: null });
      await fetchAllPlans();
      onRefreshTasks();
    } catch (err) {
      alert(`Failed to update activation toggle: ${err}`);
    }
  };

  const handleConfirmSwitchActive = async () => {
    if (!conflictModal.targetPlan) return;
    await handleToggleActive(conflictModal.targetPlan, true);
  };

  const handleCreatePlan = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!projectId) return;
    setIsCreating(true);
    try {
      const plan = await coordinatorApi.createMasterplan(
        projectId,
        newTitle.trim() || 'Masterplan',
        newRawText.trim(),
        Number(newTargetSteps),
        Number(newMaxStepsPerAgent),
        newActivate
      );
      setShowCreateModal(false);
      setNewTitle('');
      setNewRawText('');
      setNewTargetSteps(20);
      setNewMaxStepsPerAgent(4);
      setNewActivate(true);
      await fetchAllPlans();
      onRefreshTasks();
      handleOpenPlan(plan);
    } catch (err) {
      alert(`Failed to create masterplan: ${err}`);
    } finally {
      setIsCreating(false);
    }
  };

  const handleDeletePlan = async (plan: Masterplan) => {
    if (!window.confirm(`Are you sure you want to delete masterplan "${plan.title}"? This cannot be undone.`)) {
      return;
    }
    try {
      await coordinatorApi.deleteMasterplan(plan.id);
      if (activePlanId === plan.id) {
        handleBackToCatalog();
      } else {
        await fetchAllPlans();
      }
      onRefreshTasks();
    } catch (err) {
      alert(`Failed to delete masterplan: ${err}`);
    }
  };

  const handleToggleMilestoneApproval = async (enabled: boolean) => {
    if (!projectId) return;
    setIsUpdatingMode(true);
    setRequireMilestoneApproval(enabled);
    try {
      await coordinatorApi.setMasterplanMilestoneApproval(projectId, enabled);
      await fetchAllPlans();
    } catch (err) {
      console.error('Failed to update milestone mode:', err);
      setRequireMilestoneApproval(!enabled);
    } finally {
      setIsUpdatingMode(false);
    }
  };

  useEffect(() => {
    fetchAllPlans();
  }, [projectId]);

  // Real-time event polling
  useEffect(() => {
    let isMounted = true;

    const pollEvents = async () => {
      if (!projectId) return;
      try {
        const events = await coordinatorApi.getEventsAfter(lastSeqRef.current);
        if (!isMounted) return;
        if (events && events.length > 0) {
          const maxSeq = Math.max(...events.map((e) => e.sequence));
          if (maxSeq > lastSeqRef.current) {
            lastSeqRef.current = maxSeq;
          }
          const hasRelevantEvent = events.some(
            (e) =>
              e.event_type.startsWith('MASTERPLAN_') ||
              e.event_type.startsWith('TASK_') ||
              e.event_type.startsWith('STEP_') ||
              e.event_type.startsWith('AGENT_')
          );
          if (hasRelevantEvent) {
            await fetchAllPlans();
            onRefreshTasks();
          }
        }
      } catch (err) {
        // Ignored
      }
    };

    const interval = setInterval(pollEvents, 1000);
    return () => {
      isMounted = false;
      clearInterval(interval);
    };
  }, [projectId, activePlanId]);

  useEffect(() => {
    if (agents.length > 0 && !selectedAgentId) {
      setSelectedAgentId(agents[0].id);
    }
  }, [agents, selectedAgentId]);

  const handleSavePlan = async () => {
    if (!rawText.trim() || !projectId) return;
    setIsSaving(true);
    try {
      const saved = await coordinatorApi.saveMasterplan(
        projectId,
        rawText.trim(),
        Number(targetStepCount),
        Number(maxStepsPerAgent)
      );
      setSelectedPlan(saved);
      setSteps([]);
      setIsEditing(false);
      await fetchAllPlans();
      onRefreshTasks();
    } catch (err) {
      alert(`Error saving masterplan: ${err}`);
    } finally {
      setIsSaving(false);
    }
  };

  const handleResetPlan = async () => {
    if (!window.confirm('Reset this masterplan? All decomposed steps will be removed, active tasks cancelled, temporary worktrees wiped, and the Git repository cleanly reset to HEAD.')) {
      return;
    }
    try {
      await coordinatorApi.resetMasterplan(projectId, selectedPlan?.id);
      setActivePlanId(null);
      setSelectedPlan(null);
      setSteps([]);
      setIsEditing(true);
      await fetchAllPlans();
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
      await fetchAllPlans();
      onRefreshTasks();
    } catch (err) {
      alert(`Error claiming chunk: ${err}`);
    } finally {
      setIsClaiming(false);
    }
  };

  const handleParseStructuredSteps = async () => {
    if (!rawText.trim()) return;
    const lines = rawText
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => l.length > 0 && !l.startsWith('#'));
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
      await coordinatorApi.decomposeMasterplan(projectId, generatedSteps);
      setIsEditing(false);
      await fetchAllPlans();
      onRefreshTasks();
    } catch (err) {
      alert(`Failed to decompose: ${err}`);
    }
  };

  const handleCopyMcpPrompt = () => {
    const projName = project?.name || 'Project';
    const projPath = project?.path || 'repository';
    const targetSteps = targetStepCount || 20;
    const p1End = Math.max(1, Math.round(targetSteps * 0.25));
    const p2End = Math.max(p1End + 1, Math.round(targetSteps * 0.5));
    const p3End = Math.max(p2End + 1, Math.round(targetSteps * 0.75));

    const prompt = `Role: Lead Architect & AI Planner
Project: ${projName} (ID: ${projectId})
Repository Path: ${projPath}
Target Step Count: ${targetSteps} Execution Steps

Decompose the masterplan blueprint into ${targetSteps} exhaustive, production-grade milestones using MCP tool: masterplan_decompose.

Decomposition Strategy (4-Phase Architecture):
1. Phase 1 (Steps 1–${p1End}): Core Foundation, Database Schemas, Shared Types & Interfaces, Utilities, and Infrastructure.
2. Phase 2 (Steps ${p1End + 1}–${p2End}): Domain Business Logic, State Stores, Service Layers, APIs, IPC Handlers, and Workflows.
3. Phase 3 (Steps ${p2End + 1}–${p3End}): High-Fidelity UI Components, Responsive Layouts, Glassmorphism Styling, Spatial Motion, Keyboard Navigation, and Error Boundaries.
4. Phase 4 (Steps ${p3End + 1}–${targetSteps}): Integration, Edge-Case Handling, Automated Verification Suites, and Final Step ${targetSteps}: Build Production Executable/Bundle, Create Automated Launcher Script (run.bat / start.sh), Test Launch, and Write Complete USER_GUIDE.md.

Deep Specification Requirements:
- For every step, provide deep, rich specifications: Exact Target Files, Concrete Interfaces, State Transitions, Non-Overlapping Scopes, and Test Verification.
- You can submit in phased 25-step chunks using:
  masterplan_decompose(project_id="${projectId}", steps=[...], append=true)
  or all steps at once.`;

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
        return (
          <span
            className="badge"
            style={{
              backgroundColor: 'rgba(210, 153, 34, 0.2)',
              color: 'var(--accent-yellow)',
              border: '1px solid rgba(210, 153, 34, 0.4)',
            }}
          >
            UNSORTED
          </span>
        );
      case 'RESORTED':
        return (
          <span
            className="badge"
            style={{
              backgroundColor: 'rgba(56, 139, 253, 0.2)',
              color: 'var(--accent-blue)',
              border: '1px solid rgba(56, 139, 253, 0.4)',
            }}
          >
            ORGANIZED & READY
          </span>
        );
      case 'EXECUTING':
        return <span className="badge badge-RUNNING">EXECUTING</span>;
      case 'COMPLETED':
        return <span className="badge badge-READY">COMPLETED</span>;
      default:
        return <span className="badge">{status}</span>;
    }
  };

  // Step options up to 100
  const stepCountOptions = [5, 10, 15, 20, 25, 30, 40, 50, 60, 75, 100];
  // Max steps per agent options up to 8
  const maxStepsOptions = [1, 2, 3, 4, 5, 6, 7, 8];

  const activeMasterplan = masterplans.find((p) => p.is_active);

  // Render Loading
  if (loading) {
    return (
      <div className="view-content animate-fade-in" style={{ padding: '3rem', textAlign: 'center' }}>
        <div style={{ color: 'var(--text-muted)' }}>Loading Masterplans...</div>
      </div>
    );
  }

  // ==========================================
  // VIEW 1: Masterplans Catalog & Selection Grid
  // ==========================================
  if (!activePlanId || !selectedPlan) {
    return (
      <div className="view-content" style={{ flex: 1, height: '100%', overflowY: 'auto', overflowX: 'hidden' }}>
        <div className="animate-fade-in" style={{ padding: '1.5rem 2rem', maxWidth: '1400px', margin: '0 auto', width: '100%' }}>
          {/* Top Header */}
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: '1.75rem',
              paddingBottom: '1.25rem',
            borderBottom: '1px solid var(--border-color)',
          }}
        >
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem', marginBottom: '0.25rem' }}>
              <div
                style={{
                  width: '32px',
                  height: '32px',
                  borderRadius: '6px',
                  background: 'rgba(56, 139, 253, 0.15)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  color: 'var(--accent-blue)',
                }}
              >
                <Layers size={18} />
              </div>
              <h2 style={{ margin: 0, fontSize: '1.4rem', fontWeight: 600 }}>Masterplan Catalog & Swarm Control</h2>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: '1rem', color: 'var(--text-muted)', fontSize: '0.85rem' }}>
              <span>
                Project: <strong style={{ color: 'var(--text-normal)' }}>{project?.name || projectId}</strong>
              </span>
              <span>•</span>
              <span>{masterplans.length} Masterplan{masterplans.length === 1 ? '' : 's'}</span>
              <span>•</span>
              {activeMasterplan ? (
                <span style={{ display: 'inline-flex', alignItems: 'center', gap: '0.35rem', color: 'var(--accent-green)' }}>
                  <Radio size={13} className="animate-pulse" /> Active Plan: <strong>{activeMasterplan.title}</strong>
                </span>
              ) : (
                <span style={{ display: 'inline-flex', alignItems: 'center', gap: '0.35rem', color: 'var(--accent-yellow)' }}>
                  <AlertCircle size={13} /> No Active Plan (MCP Swarms Inactive)
                </span>
              )}
            </div>
          </div>

          <div style={{ display: 'flex', gap: '0.75rem' }}>
            <button
              onClick={() => setShowCreateModal(true)}
              className="btn btn-primary"
              style={{ display: 'flex', alignItems: 'center', gap: '0.5rem', padding: '0.5rem 1rem' }}
            >
              <Plus size={16} />
              New Masterplan
            </button>
          </div>
        </div>

        {/* Masterplans Grid */}
        {masterplans.length === 0 ? (
          <div
            className="card"
            style={{
              padding: '3.5rem 2rem',
              textAlign: 'center',
              background: 'var(--bg-secondary)',
              border: '1px dashed var(--border-color)',
            }}
          >
            <Layers size={48} style={{ color: 'var(--text-muted)', margin: '0 auto 1rem', opacity: 0.5 }} />
            <h3 style={{ margin: '0 0 0.5rem', fontSize: '1.2rem' }}>No Masterplans Created Yet</h3>
            <p style={{ color: 'var(--text-muted)', maxWidth: '500px', margin: '0 auto 1.5rem', fontSize: '0.9rem' }}>
              Create a masterplan to decompose your architecture into high-fidelity, verified steps and parallelize agent swarms with isolated Git worktrees.
            </p>
            <button
              onClick={() => setShowCreateModal(true)}
              className="btn btn-primary"
              style={{ display: 'inline-flex', alignItems: 'center', gap: '0.5rem' }}
            >
              <Plus size={16} />
              Create First Masterplan
            </button>
          </div>
        ) : (
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fill, minmax(380px, 1fr))',
              gap: '1.25rem',
            }}
          >
            {masterplans.map((plan) => (
              <div
                key={plan.id}
                className="card animate-fade-in"
                style={{
                  padding: '1.25rem',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '1rem',
                  border: plan.is_active
                    ? '1px solid rgba(46, 160, 67, 0.6)'
                    : '1px solid var(--border-color)',
                  boxShadow: plan.is_active
                    ? '0 0 16px rgba(46, 160, 67, 0.15)'
                    : 'none',
                  background: 'var(--bg-secondary)',
                  position: 'relative',
                }}
              >
                {/* Card Top: Title & Status */}
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                  <div style={{ flex: 1, marginRight: '0.5rem' }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
                      <h3 style={{ margin: 0, fontSize: '1.05rem', fontWeight: 600, color: 'var(--text-normal)' }}>
                        {plan.title}
                      </h3>
                    </div>
                    <div style={{ fontSize: '0.75rem', color: 'var(--text-muted)', marginTop: '0.2rem' }}>
                      Updated {new Date(plan.updated_at).toLocaleDateString()} at {new Date(plan.updated_at).toLocaleTimeString()}
                    </div>
                  </div>
                  {getStatusBadge(plan.status)}
                </div>

                {/* Active / Published Toggle Switch */}
                <div
                  style={{
                    padding: '0.65rem 0.85rem',
                    borderRadius: '6px',
                    backgroundColor: plan.is_active ? 'rgba(46, 160, 67, 0.1)' : 'rgba(255, 255, 255, 0.03)',
                    border: plan.is_active ? '1px solid rgba(46, 160, 67, 0.3)' : '1px solid var(--border-color)',
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                  }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
                    {plan.is_active ? (
                      <Eye size={15} style={{ color: 'var(--accent-green)' }} />
                    ) : (
                      <EyeOff size={15} style={{ color: 'var(--text-muted)' }} />
                    )}
                    <div>
                      <div style={{ fontSize: '0.8rem', fontWeight: 600, color: plan.is_active ? 'var(--accent-green)' : 'var(--text-muted)' }}>
                        {plan.is_active ? 'ACTIVE — Visible to AI Agents' : 'INACTIVE — Hidden from MCP'}
                      </div>
                      <div style={{ fontSize: '0.7rem', color: 'var(--text-muted)' }}>
                        {plan.is_active ? 'Agents can claim chunks and execute steps' : 'Agents cannot see or claim this masterplan'}
                      </div>
                    </div>
                  </div>

                  <button
                    onClick={() => handleToggleActive(plan)}
                    className={plan.is_active ? 'btn btn-secondary' : 'btn btn-primary'}
                    style={{
                      fontSize: '0.75rem',
                      padding: '0.25rem 0.65rem',
                      height: 'auto',
                      minHeight: 'unset',
                    }}
                  >
                    {plan.is_active ? 'Deactivate' : 'Activate Plan'}
                  </button>
                </div>

                {/* Plan Meta Chips */}
                <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap', fontSize: '0.75rem' }}>
                  <span
                    style={{
                      padding: '0.2rem 0.5rem',
                      borderRadius: '4px',
                      background: 'rgba(255, 255, 255, 0.05)',
                      color: 'var(--text-muted)',
                      border: '1px solid var(--border-color)',
                    }}
                  >
                    Target: <strong>{plan.target_step_count} steps</strong>
                  </span>
                  <span
                    style={{
                      padding: '0.2rem 0.5rem',
                      borderRadius: '4px',
                      background: 'rgba(255, 255, 255, 0.05)',
                      color: 'var(--text-muted)',
                      border: '1px solid var(--border-color)',
                    }}
                  >
                    Chunk Cap: <strong>Max {plan.max_steps_per_agent} / agent</strong>
                  </span>
                  <span
                    style={{
                      padding: '0.2rem 0.5rem',
                      borderRadius: '4px',
                      background: 'rgba(255, 255, 255, 0.05)',
                      color: 'var(--text-muted)',
                      border: '1px solid var(--border-color)',
                    }}
                  >
                    Mode: <strong>{plan.require_milestone_approval ? 'Milestones' : 'Autonomous'}</strong>
                  </span>
                </div>

                {/* Card Actions */}
                <div
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                    marginTop: 'auto',
                    paddingTop: '0.75rem',
                    borderTop: '1px solid var(--border-color)',
                  }}
                >
                  <button
                    onClick={() => handleOpenPlan(plan)}
                    className="btn btn-secondary"
                    style={{ display: 'flex', alignItems: 'center', gap: '0.4rem', fontSize: '0.8rem' }}
                  >
                    <Edit3 size={13} />
                    Inspect & Decompose
                  </button>

                  <button
                    onClick={() => handleDeletePlan(plan)}
                    className="btn btn-secondary"
                    title="Delete Masterplan"
                    style={{
                      padding: '0.4rem',
                      color: 'var(--accent-red)',
                      borderColor: 'rgba(248, 81, 73, 0.3)',
                    }}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Modal: Create New Masterplan */}
        {showCreateModal && (
          <div
            style={{
              position: 'fixed',
              top: 0,
              left: 0,
              right: 0,
              bottom: 0,
              background: 'rgba(0, 0, 0, 0.75)',
              backdropFilter: 'blur(4px)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              zIndex: 1000,
              padding: '1rem',
            }}
          >
            <div
              className="card animate-scale-up"
              style={{
                width: '100%',
                maxWidth: '650px',
                background: 'var(--bg-primary)',
                border: '1px solid var(--border-color)',
                borderRadius: '8px',
                padding: '1.75rem',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '1.25rem' }}>
                <h3 style={{ margin: 0, fontSize: '1.2rem', fontWeight: 600, display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
                  <Layers size={18} style={{ color: 'var(--accent-blue)' }} /> Create New Masterplan
                </h3>
                <button
                  onClick={() => setShowCreateModal(false)}
                  className="btn btn-secondary"
                  style={{ padding: '0.25rem 0.5rem', fontSize: '0.8rem' }}
                >
                  ✕
                </button>
              </div>

              <form onSubmit={handleCreatePlan} style={{ display: 'flex', flexDirection: 'column', gap: '1.2rem' }}>
                <div>
                  <label style={{ display: 'block', fontSize: '0.85rem', fontWeight: 600, marginBottom: '0.4rem' }}>
                    Masterplan Title
                  </label>
                  <input
                    type="text"
                    className="input"
                    value={newTitle}
                    onChange={(e) => setNewTitle(e.target.value)}
                    placeholder="e.g. Phase 1: Authentication & Database Architecture"
                    required
                    style={{ width: '100%' }}
                  />
                </div>

                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '1rem' }}>
                  <div>
                    <label style={{ display: 'block', fontSize: '0.85rem', fontWeight: 600, marginBottom: '0.4rem' }}>
                      Target Step Count (Up to 100)
                    </label>
                    <select
                      className="input"
                      value={newTargetSteps}
                      onChange={(e) => setNewTargetSteps(Number(e.target.value))}
                      style={{ width: '100%' }}
                    >
                      {stepCountOptions.map((n) => (
                        <option key={n} value={n}>
                          {n} Steps
                        </option>
                      ))}
                    </select>
                  </div>

                  <div>
                    <label style={{ display: 'block', fontSize: '0.85rem', fontWeight: 600, marginBottom: '0.4rem' }}>
                      Max Steps Per Agent (Up to 8)
                    </label>
                    <select
                      className="input"
                      value={newMaxStepsPerAgent}
                      onChange={(e) => setNewMaxStepsPerAgent(Number(e.target.value))}
                      style={{ width: '100%' }}
                    >
                      {maxStepsOptions.map((n) => (
                        <option key={n} value={n}>
                          {n} Steps per Agent
                        </option>
                      ))}
                    </select>
                  </div>
                </div>

                <div>
                  <label style={{ display: 'block', fontSize: '0.85rem', fontWeight: 600, marginBottom: '0.4rem' }}>
                    Raw Specification / Architectural Blueprint
                  </label>
                  <textarea
                    className="input"
                    value={newRawText}
                    onChange={(e) => setNewRawText(e.target.value)}
                    placeholder="Paste or write raw specification requirements here. The architect agent will decompose this into structured execution milestones."
                    rows={8}
                    style={{ width: '100%', resize: 'vertical', fontFamily: 'var(--font-mono)', fontSize: '0.85rem' }}
                  />
                </div>

                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: '0.75rem',
                    padding: '0.75rem',
                    borderRadius: '6px',
                    background: 'var(--bg-secondary)',
                    border: '1px solid var(--border-color)',
                  }}
                >
                  <input
                    type="checkbox"
                    id="newActivate"
                    checked={newActivate}
                    onChange={(e) => setNewActivate(e.target.checked)}
                    style={{ width: '16px', height: '16px', cursor: 'pointer' }}
                  />
                  <label htmlFor="newActivate" style={{ fontSize: '0.85rem', cursor: 'pointer' }}>
                    <strong>Activate immediately</strong> (sets this as active plan for MCP agents; deactivates any other active plan)
                  </label>
                </div>

                <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '0.75rem', marginTop: '0.5rem' }}>
                  <button
                    type="button"
                    onClick={() => setShowCreateModal(false)}
                    className="btn btn-secondary"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    className="btn btn-primary"
                    disabled={isCreating}
                    style={{ display: 'flex', alignItems: 'center', gap: '0.4rem' }}
                  >
                    {isCreating ? 'Creating...' : 'Create Masterplan'}
                  </button>
                </div>
              </form>
            </div>
          </div>
        )}

        {/* Conflict Modal: Single Active Invariant Warning */}
        {conflictModal.show && conflictModal.targetPlan && conflictModal.currentActivePlan && (
          <div
            style={{
              position: 'fixed',
              top: 0,
              left: 0,
              right: 0,
              bottom: 0,
              background: 'rgba(0, 0, 0, 0.8)',
              backdropFilter: 'blur(4px)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              zIndex: 1100,
              padding: '1rem',
            }}
          >
            <div
              className="card animate-scale-up"
              style={{
                width: '100%',
                maxWidth: '520px',
                background: 'var(--bg-primary)',
                border: '1px solid rgba(210, 153, 34, 0.4)',
                borderRadius: '8px',
                padding: '1.75rem',
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem', marginBottom: '1rem' }}>
                <div
                  style={{
                    width: '36px',
                    height: '36px',
                    borderRadius: '50%',
                    background: 'rgba(210, 153, 34, 0.15)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    color: 'var(--accent-yellow)',
                  }}
                >
                  <AlertTriangle size={20} />
                </div>
                <h3 style={{ margin: 0, fontSize: '1.15rem', fontWeight: 600 }}>Masterplan Activation Conflict</h3>
              </div>

              <p style={{ fontSize: '0.9rem', color: 'var(--text-muted)', lineHeight: '1.5', margin: '0 0 1.25rem' }}>
                Only <strong>one masterplan</strong> can be active at a time for this project. Masterplan{' '}
                <strong style={{ color: 'var(--text-normal)' }}>"{conflictModal.currentActivePlan.title}"</strong> is currently active and receiving agent traffic.
              </p>

              <div
                style={{
                  padding: '0.75rem',
                  borderRadius: '6px',
                  backgroundColor: 'rgba(56, 139, 253, 0.08)',
                  border: '1px solid rgba(56, 139, 253, 0.2)',
                  fontSize: '0.85rem',
                  color: 'var(--text-normal)',
                  marginBottom: '1.5rem',
                }}
              >
                Activating <strong>"{conflictModal.targetPlan.title}"</strong> will immediately deactivate "{conflictModal.currentActivePlan.title}" and redirect all AI agent MCP requests to the new plan.
              </div>

              <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '0.75rem' }}>
                <button
                  onClick={() => setConflictModal({ show: false, targetPlan: null, currentActivePlan: null })}
                  className="btn btn-secondary"
                >
                  Cancel
                </button>
                <button
                  onClick={handleConfirmSwitchActive}
                  className="btn btn-primary"
                  style={{ background: 'var(--accent-yellow)', color: '#000', borderColor: 'var(--accent-yellow)' }}
                >
                  Switch Active Masterplan
                </button>
              </div>
            </div>
          </div>
        )}
        </div>
      </div>
    );
  }

  // ==========================================
  // VIEW 2: Detailed Masterplan Inspection & Step Hub
  // ==========================================
  return (
    <div className="view-content" style={{ flex: 1, height: '100%', overflowY: 'auto', overflowX: 'hidden' }}>
      <div className="animate-fade-in" style={{ padding: '1.5rem 2rem', maxWidth: '1400px', margin: '0 auto', width: '100%' }}>
        {/* Back to Catalog Bar */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '1.25rem' }}>
        <button
          onClick={handleBackToCatalog}
          className="btn btn-secondary"
          style={{ display: 'flex', alignItems: 'center', gap: '0.4rem', fontSize: '0.85rem' }}
        >
          <ArrowLeft size={15} />
          Back to Masterplans Catalog
        </button>

        <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem' }}>
          <div
            style={{
              padding: '0.35rem 0.75rem',
              borderRadius: '6px',
              backgroundColor: selectedPlan.is_active ? 'rgba(46, 160, 67, 0.15)' : 'rgba(255, 255, 255, 0.05)',
              border: selectedPlan.is_active ? '1px solid rgba(46, 160, 67, 0.4)' : '1px solid var(--border-color)',
              display: 'flex',
              alignItems: 'center',
              gap: '0.5rem',
              fontSize: '0.8rem',
            }}
          >
            {selectedPlan.is_active ? (
              <Eye size={14} style={{ color: 'var(--accent-green)' }} />
            ) : (
              <EyeOff size={14} style={{ color: 'var(--text-muted)' }} />
            )}
            <span style={{ color: selectedPlan.is_active ? 'var(--accent-green)' : 'var(--text-muted)', fontWeight: 600 }}>
              {selectedPlan.is_active ? 'Active on MCP' : 'Inactive (Hidden)'}
            </span>
            <button
              onClick={() => handleToggleActive(selectedPlan)}
              className="btn btn-secondary"
              style={{ fontSize: '0.7rem', padding: '0.15rem 0.5rem', height: 'auto', minHeight: 'unset' }}
            >
              {selectedPlan.is_active ? 'Deactivate' : 'Activate'}
            </button>
          </div>

          <button
            onClick={() => handleDeletePlan(selectedPlan)}
            className="btn btn-secondary"
            style={{ color: 'var(--accent-red)', borderColor: 'rgba(248, 81, 73, 0.3)', fontSize: '0.8rem' }}
          >
            <Trash2 size={14} />
          </button>
        </div>
      </div>

      {/* Header Banner */}
      <div
        className="card"
        style={{
          padding: '1.25rem 1.5rem',
          marginBottom: '1.25rem',
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          background: 'var(--bg-secondary)',
          border: '1px solid var(--border-color)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '1rem' }}>
          <div
            style={{
              width: '40px',
              height: '40px',
              borderRadius: '8px',
              background: 'rgba(56, 139, 253, 0.15)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              color: 'var(--accent-blue)',
            }}
          >
            <Layers size={22} />
          </div>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem' }}>
              <h2 style={{ margin: 0, fontSize: '1.3rem', fontWeight: 600 }}>{selectedPlan.title}</h2>
              {getStatusBadge(selectedPlan.status)}
            </div>
            <div style={{ display: 'flex', gap: '1rem', color: 'var(--text-muted)', fontSize: '0.8rem', marginTop: '0.25rem' }}>
              <span>
                Project: <strong>{project?.name || projectId}</strong>
              </span>
              <span>•</span>
              <span>
                Target: <strong>{selectedPlan.target_step_count} steps</strong>
              </span>
              <span>•</span>
              <span>
                Chunk Cap: <strong>Max {selectedPlan.max_steps_per_agent} / agent</strong>
              </span>
            </div>
          </div>
        </div>

        <div style={{ display: 'flex', gap: '0.5rem' }}>
          <button
            onClick={handleCopyMcpPrompt}
            className="btn btn-secondary"
            style={{ display: 'flex', alignItems: 'center', gap: '0.4rem', fontSize: '0.8rem' }}
          >
            {copiedPrompt ? <Check size={14} style={{ color: 'var(--accent-green)' }} /> : <Copy size={14} />}
            {copiedPrompt ? 'Prompt Copied!' : 'Copy Handoff Prompt'}
          </button>
          <button
            onClick={handleResetPlan}
            className="btn btn-secondary"
            style={{ display: 'flex', alignItems: 'center', gap: '0.4rem', fontSize: '0.8rem', color: 'var(--accent-yellow)' }}
          >
            <RotateCcw size={14} />
            Reset Plan
          </button>
        </div>
      </div>

      {/* Hybrid Milestone Approval Control Banner */}
      <div
        className="card"
        style={{
          padding: '0.9rem 1.25rem',
          marginBottom: '1.25rem',
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          background: requireMilestoneApproval
            ? 'rgba(56, 139, 253, 0.06)'
            : 'rgba(210, 153, 34, 0.06)',
          border: requireMilestoneApproval
            ? '1px solid rgba(56, 139, 253, 0.3)'
            : '1px solid rgba(210, 153, 34, 0.3)',
          borderRadius: '6px',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '0.85rem' }}>
          <div
            style={{
              width: '32px',
              height: '32px',
              borderRadius: '6px',
              background: requireMilestoneApproval ? 'rgba(56, 139, 253, 0.15)' : 'rgba(210, 153, 34, 0.15)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              color: requireMilestoneApproval ? 'var(--accent-blue)' : 'var(--accent-yellow)',
            }}
          >
            {requireMilestoneApproval ? <Bot size={18} /> : <Zap size={18} />}
          </div>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
              <span style={{ fontSize: '0.85rem', fontWeight: 600 }}>
                {requireMilestoneApproval
                  ? 'Interactive Milestone Checkpoints Active'
                  : 'Continuous Autonomous Swarm Mode Active'}
              </span>
              <span
                style={{
                  fontSize: '0.7rem',
                  padding: '0.15rem 0.45rem',
                  borderRadius: '4px',
                  fontWeight: 600,
                  backgroundColor: requireMilestoneApproval ? 'rgba(56, 139, 253, 0.2)' : 'rgba(210, 153, 34, 0.2)',
                  color: requireMilestoneApproval ? 'var(--accent-blue)' : 'var(--accent-yellow)',
                }}
              >
                {requireMilestoneApproval ? 'PAUSE & REPORT' : 'UNINTERRUPTED SWARM'}
              </span>
            </div>
            <div style={{ fontSize: '0.75rem', color: 'var(--text-muted)', marginTop: '0.15rem' }}>
              {requireMilestoneApproval
                ? 'Agents submit their completed chunk, stop tool calls, report in IDE chat, and wait for confirmation before claiming the next chunk.'
                : 'Agents submit their completed chunk and immediately claim subsequent chunks autonomously until all steps finish.'}
            </div>
          </div>
        </div>

        <button
          onClick={() => handleToggleMilestoneApproval(!requireMilestoneApproval)}
          disabled={isUpdatingMode}
          className="btn btn-secondary"
          style={{
            fontSize: '0.8rem',
            padding: '0.35rem 0.75rem',
            borderColor: requireMilestoneApproval ? 'rgba(56, 139, 253, 0.4)' : 'rgba(210, 153, 34, 0.4)',
          }}
        >
          {isUpdatingMode
            ? 'Updating...'
            : requireMilestoneApproval
            ? 'Switch to Autonomous Swarm'
            : 'Switch to Interactive Milestones'}
        </button>
      </div>

      {/* Raw Specification Editor if UNSORTED or isEditing */}
      {(selectedPlan.status === 'UNSORTED' || isEditing) && (
        <div className="card" style={{ padding: '1.25rem', marginBottom: '1.25rem', background: 'var(--bg-secondary)' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '0.75rem' }}>
            <h3 style={{ margin: 0, fontSize: '1rem', fontWeight: 600, display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
              <FileText size={16} /> Raw Master Specification
            </h3>
            <div style={{ display: 'flex', gap: '0.75rem', alignItems: 'center' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '0.4rem', fontSize: '0.8rem' }}>
                <span style={{ color: 'var(--text-muted)' }}>Target:</span>
                <select
                  className="input"
                  value={targetStepCount}
                  onChange={(e) => setTargetStepCount(Number(e.target.value))}
                  style={{ padding: '0.2rem 0.5rem', fontSize: '0.8rem' }}
                >
                  {stepCountOptions.map((n) => (
                    <option key={n} value={n}>
                      {n} Steps
                    </option>
                  ))}
                </select>
              </div>

              <div style={{ display: 'flex', alignItems: 'center', gap: '0.4rem', fontSize: '0.8rem' }}>
                <span style={{ color: 'var(--text-muted)' }}>Chunk Cap:</span>
                <select
                  className="input"
                  value={maxStepsPerAgent}
                  onChange={(e) => setMaxStepsPerAgent(Number(e.target.value))}
                  style={{ padding: '0.2rem 0.5rem', fontSize: '0.8rem' }}
                >
                  {maxStepsOptions.map((n) => (
                    <option key={n} value={n}>
                      {n} Steps
                    </option>
                  ))}
                </select>
              </div>

              <button
                onClick={handleSavePlan}
                disabled={isSaving}
                className="btn btn-primary"
                style={{ fontSize: '0.8rem', padding: '0.35rem 0.75rem' }}
              >
                {isSaving ? 'Saving...' : 'Save Specification'}
              </button>
            </div>
          </div>

          <textarea
            className="input"
            value={rawText}
            onChange={(e) => setRawText(e.target.value)}
            placeholder="Paste raw masterplan specifications here..."
            rows={10}
            style={{ width: '100%', resize: 'vertical', fontFamily: 'var(--font-mono)', fontSize: '0.85rem' }}
          />

          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: '0.75rem' }}>
            <span style={{ fontSize: '0.75rem', color: 'var(--text-muted)' }}>
              Tip: AI agents calling <code>masterplan_decompose</code> will automatically formulate high-fidelity steps matching this spec.
            </span>
            <button
              onClick={handleParseStructuredSteps}
              className="btn btn-secondary"
              style={{ fontSize: '0.8rem', display: 'flex', alignItems: 'center', gap: '0.4rem' }}
            >
              <Sparkles size={14} style={{ color: 'var(--accent-blue)' }} /> Quick Auto-Decompose (Client)
            </button>
          </div>
        </div>
      )}

      {/* Progress & Step Management when organized */}
      {selectedPlan.status !== 'UNSORTED' && (
        <>
          {/* Progress Bar Card */}
          <div
            className="card"
            style={{
              padding: '1.25rem',
              marginBottom: '1.25rem',
              background: 'var(--bg-secondary)',
              border: '1px solid var(--border-color)',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '0.6rem' }}>
              <span style={{ fontSize: '0.85rem', fontWeight: 600 }}>Masterplan Execution Progress</span>
              <span style={{ fontSize: '0.85rem', color: 'var(--text-muted)' }}>
                <strong>{completedSteps}</strong> of <strong>{totalSteps}</strong> Steps Merged ({progressPercent}%)
              </span>
            </div>
            <div
              style={{
                width: '100%',
                height: '8px',
                background: 'rgba(255, 255, 255, 0.08)',
                borderRadius: '4px',
                overflow: 'hidden',
              }}
            >
              <div
                style={{
                  width: `${progressPercent}%`,
                  height: '100%',
                  background: progressPercent === 100 ? 'var(--accent-green)' : 'var(--accent-blue)',
                  borderRadius: '4px',
                  transition: 'width 0.3s ease',
                }}
              />
            </div>

            {/* Step Status Badges Counter */}
            <div style={{ display: 'flex', gap: '1rem', marginTop: '0.75rem', fontSize: '0.8rem' }}>
              <span style={{ color: 'var(--text-muted)' }}>
                Pending: <strong style={{ color: 'var(--text-normal)' }}>{pendingSteps}</strong>
              </span>
              <span style={{ color: 'var(--text-muted)' }}>
                Claimed / In-Progress: <strong style={{ color: 'var(--accent-blue)' }}>{claimedSteps}</strong>
              </span>
              <span style={{ color: 'var(--text-muted)' }}>
                Completed: <strong style={{ color: 'var(--accent-green)' }}>{completedSteps}</strong>
              </span>
            </div>
          </div>

          {/* Step Filter Bar & Manual Claim Control */}
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: '1rem',
              gap: '1rem',
              flexWrap: 'wrap',
            }}
          >
            {/* Filter Tabs */}
            <div style={{ display: 'flex', gap: '0.4rem' }}>
              {(['ALL', 'PENDING', 'CLAIMED', 'COMPLETED'] as const).map((f) => (
                <button
                  key={f}
                  onClick={() => setFilter(f)}
                  className={`btn ${filter === f ? 'btn-primary' : 'btn-secondary'}`}
                  style={{ fontSize: '0.75rem', padding: '0.3rem 0.75rem' }}
                >
                  {f} {f === 'ALL' ? `(${totalSteps})` : f === 'PENDING' ? `(${pendingSteps})` : f === 'CLAIMED' ? `(${claimedSteps})` : `(${completedSteps})`}
                </button>
              ))}
            </div>

            {/* Manual Test Claim Controls */}
            <div style={{ display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
              <select
                className="input"
                value={selectedAgentId}
                onChange={(e) => setSelectedAgentId(e.target.value)}
                style={{ fontSize: '0.8rem', padding: '0.3rem 0.5rem' }}
              >
                {agents.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name} ({a.agent_type})
                  </option>
                ))}
              </select>

              <select
                className="input"
                value={customChunkSize}
                onChange={(e) => setCustomChunkSize(Number(e.target.value))}
                style={{ fontSize: '0.8rem', padding: '0.3rem 0.5rem' }}
              >
                {maxStepsOptions.map((n) => (
                  <option key={n} value={n}>
                    {n} Steps Chunk
                  </option>
                ))}
              </select>

              <button
                onClick={handleClaimChunk}
                disabled={isClaiming || pendingSteps === 0}
                className="btn btn-primary"
                style={{ display: 'flex', alignItems: 'center', gap: '0.35rem', fontSize: '0.8rem', padding: '0.35rem 0.75rem' }}
              >
                <Play size={13} />
                {isClaiming ? 'Claiming...' : 'Manual Claim Chunk'}
              </button>

              {claimedSteps > 0 && (
                <button
                  onClick={async () => {
                    if (window.confirm(`Unclaim all ${claimedSteps} active step(s)? All in-flight chunk tasks will be cancelled, worktrees cleaned, and steps reverted to PENDING.`)) {
                      try {
                        const activeTasks = Array.from(new Set(steps.filter(s => s.claimed_task_id && s.status !== 'COMPLETED').map(s => s.claimed_task_id!)));
                        for (const tid of activeTasks) {
                          await coordinatorApi.requeueTask(tid);
                        }
                        await fetchAllPlans();
                        onRefreshTasks();
                      } catch (err) {
                        alert(`Failed to unclaim steps: ${err}`);
                      }
                    }
                  }}
                  className="btn btn-secondary"
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: '0.35rem',
                    fontSize: '0.8rem',
                    padding: '0.35rem 0.75rem',
                    color: 'var(--accent-yellow)',
                    borderColor: 'rgba(240, 140, 0, 0.4)',
                  }}
                  title="Revert all currently claimed steps back to PENDING"
                >
                  Unclaim All ({claimedSteps})
                </button>
              )}
            </div>
          </div>

          {/* Steps List */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: '0.75rem' }}>
            {filteredSteps.map((s) => (
              <div
                key={s.id}
                className="card animate-fade-in"
                style={{
                  padding: '1rem',
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'flex-start',
                  background: 'var(--bg-secondary)',
                  border: s.status === 'COMPLETED'
                    ? '1px solid rgba(46, 160, 67, 0.4)'
                    : s.status === 'CLAIMED' || s.status === 'IN_PROGRESS'
                    ? '1px solid rgba(56, 139, 253, 0.4)'
                    : '1px solid var(--border-color)',
                }}
              >
                <div style={{ flex: 1, marginRight: '1rem' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '0.5rem', marginBottom: '0.3rem' }}>
                    <span
                      style={{
                        fontSize: '0.75rem',
                        fontWeight: 700,
                        padding: '0.15rem 0.45rem',
                        borderRadius: '4px',
                        background: 'rgba(255, 255, 255, 0.08)',
                        color: 'var(--text-muted)',
                      }}
                    >
                      Step #{s.step_index}
                    </span>
                    <h4 style={{ margin: 0, fontSize: '0.95rem', fontWeight: 600 }}>{s.title}</h4>
                  </div>
                  <p style={{ margin: '0 0 0.5rem', fontSize: '0.85rem', color: 'var(--text-muted)', lineHeight: '1.4' }}>
                    {s.description}
                  </p>

                  <div style={{ display: 'flex', gap: '0.75rem', flexWrap: 'wrap', fontSize: '0.75rem' }}>
                    {s.suggested_scope && (
                      <span style={{ color: 'var(--text-muted)' }}>
                        Scope: <code style={{ color: 'var(--accent-blue)' }}>{s.suggested_scope}</code>
                      </span>
                    )}
                    {s.acceptance_criteria && (
                      <span style={{ color: 'var(--text-muted)' }}>
                        Criteria: <code style={{ color: 'var(--text-normal)' }}>{s.acceptance_criteria}</code>
                      </span>
                    )}
                  </div>
                </div>

                <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: '0.4rem' }}>
                  <span
                    className="badge"
                    style={{
                      backgroundColor:
                        s.status === 'COMPLETED'
                          ? 'rgba(46, 160, 67, 0.2)'
                          : s.status === 'CLAIMED' || s.status === 'IN_PROGRESS'
                          ? 'rgba(56, 139, 253, 0.2)'
                          : 'rgba(255, 255, 255, 0.08)',
                      color:
                        s.status === 'COMPLETED'
                          ? 'var(--accent-green)'
                          : s.status === 'CLAIMED' || s.status === 'IN_PROGRESS'
                          ? 'var(--accent-blue)'
                          : 'var(--text-muted)',
                      border:
                        s.status === 'COMPLETED'
                          ? '1px solid rgba(46, 160, 67, 0.4)'
                          : s.status === 'CLAIMED' || s.status === 'IN_PROGRESS'
                          ? '1px solid rgba(56, 139, 253, 0.4)'
                          : '1px solid var(--border-color)',
                    }}
                  >
                    {s.status}
                  </span>
                  {s.claimed_agent_id && (
                    <span style={{ fontSize: '0.7rem', color: 'var(--accent-blue)', fontWeight: 600 }}>
                      Agent: {s.claimed_agent_id}
                    </span>
                  )}
                  {s.claimed_task_id && s.status !== 'COMPLETED' && (
                    <button
                      className="btn btn-secondary"
                      style={{
                        height: 22,
                        padding: '0 6px',
                        fontSize: '0.7rem',
                        color: 'var(--accent-yellow)',
                        borderColor: 'rgba(240, 140, 0, 0.3)',
                      }}
                      onClick={async () => {
                        if (window.confirm(`Unclaim step #${s.step_index}? It will revert to PENDING, releasing locks and worktrees.`)) {
                          try {
                            await coordinatorApi.requeueTask(s.claimed_task_id!);
                            await fetchAllPlans();
                            onRefreshTasks();
                          } catch (err) {
                            alert(`Failed to unclaim step: ${err}`);
                          }
                        }
                      }}
                      title="Revert this step back to PENDING and release worktree/locks"
                    >
                      Unclaim
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        </>
      )}
      </div>
    </div>
  );
};
