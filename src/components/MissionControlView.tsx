import React from 'react';
import { Task, Agent, MergeQueueItem } from '../types';
import {
  Play,
  Clock,
  GitMerge,
  ShieldAlert,
  Cpu,
  BookOpen,
  Plus,
  Layers,
  ArrowRight,
} from 'lucide-react';

interface MissionControlViewProps {
  tasks: Task[];
  agents: Agent[];
  mergeQueue: MergeQueueItem[];
  onSelectTask: (t: Task) => void;
  onOpenGuide?: () => void;
  onOpenNewTask?: () => void;
  onOpenImport?: () => void;
  onNavigateTab?: (tab: string) => void;
}

export const MissionControlView: React.FC<MissionControlViewProps> = ({
  tasks,
  agents,
  mergeQueue,
  onSelectTask,
  onOpenGuide,
  onOpenNewTask,
  onNavigateTab,
}) => {
  const needsAttention = tasks.filter((t) => t.state === 'BLOCKED' || t.state === 'FAILED');
  const runningTasks = tasks.filter((t) => t.state === 'RUNNING');
  const waitingTasks = tasks.filter((t) => t.state === 'READY' || t.state === 'REVIEW');
  const completedTasks = tasks.filter((t) => t.state === 'DONE');
  const workingAgents = agents.filter((a) => a.status === 'WORKING');
  const idleAgents = agents.filter((a) => a.status === 'IDLE');

  return (
    <div style={{ flex: 1, padding: 18, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 14 }}>
      {/* Tactical Mission Header Banner */}
      <div
        style={{
          backgroundColor: 'var(--bg-surface)',
          border: '1px solid var(--border-medium)',
          borderRadius: 'var(--radius-card)',
          padding: '12px 16px',
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: 16,
          boxShadow: 'var(--shadow-tactical)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          <div
            style={{
              width: 34,
              height: 34,
              borderRadius: 'var(--radius-xs)',
              backgroundColor: 'rgba(34, 211, 238, 0.10)',
              color: 'var(--accent-telemetry)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
              border: '1px solid rgba(34, 211, 238, 0.25)',
            }}
          >
            <BookOpen size={16} strokeWidth={1.75} />
          </div>
          <div>
            <div style={{ fontWeight: 600, fontSize: 13, color: 'var(--text-primary)', letterSpacing: '0.01em' }}>
              Autonomous Multi-Agent Coordination Pipeline
            </div>
            <div style={{ fontSize: 11, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', marginTop: 2 }}>
              Connect Repo → Connect Agent → Allocate Scope → Verify → Serialize Merge
            </div>
          </div>
        </div>

        <div style={{ display: 'flex', gap: 8, flexShrink: 0 }}>
          {onNavigateTab && (
            <button
              className="btn btn-secondary"
              style={{ height: 28, fontSize: 11, fontFamily: 'var(--font-mono)' }}
              onClick={() => onNavigateTab('masterplan')}
              title="Open Masterplan Execution Hub to decompose raw specs into agent chunks"
            >
              <Layers size={13} style={{ color: 'var(--accent-primary)' }} /> Masterplan Hub
            </button>
          )}
          <button
            className="btn btn-secondary"
            style={{ height: 28, fontSize: 11 }}
            onClick={onOpenGuide}
            title="Read the full plain-English walkthrough with diagram and examples"
          >
            Workflow Guide
          </button>
          <button
            className="btn btn-primary"
            style={{ height: 28, fontSize: 11 }}
            onClick={onOpenNewTask}
            title="Create a new task with custom prompt and verification steps"
          >
            <Plus size={13} /> New Task
          </button>
        </div>
      </div>

      {/* Asymmetric Bento Telemetry Deck (DESIGN_VARIANCE: 7, VISUAL_DENSITY: 8) */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(12, 1fr)', gap: 12 }}>
        {/* Core Coordinator Node (4 Cols) */}
        <div
          style={{
            gridColumn: 'span 4',
            backgroundColor: 'var(--bg-surface)',
            border: '1px solid var(--border-medium)',
            borderRadius: 'var(--radius-card)',
            padding: '12px 14px',
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'space-between',
            boxShadow: 'var(--shadow-tactical)',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
            <span style={{ fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>
              Control Plane Engine
            </span>
            <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5, color: 'var(--accent-green)', fontSize: 11, fontFamily: 'var(--font-mono)', fontWeight: 600 }}>
              <span style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: 'var(--accent-green)' }} />
              ONLINE
            </span>
          </div>
          <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
            <span style={{ fontSize: 22, fontWeight: 700, fontFamily: 'var(--font-mono)', color: 'var(--text-primary)' }} className="tabular-nums">
              {tasks.length}
            </span>
            <span style={{ fontSize: 11, color: 'var(--text-secondary)' }}>Total Tasks Tracked</span>
          </div>
          <div style={{ borderTop: '1px solid var(--border-subtle)', marginTop: 10, paddingTop: 8, display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>
            <span>Protocol: MCP 2024-11-05</span>
            <span style={{ color: 'var(--accent-primary)' }}>Port 7890</span>
          </div>
        </div>

        {/* Agent Fleet Deck (4 Cols) */}
        <div
          style={{
            gridColumn: 'span 4',
            backgroundColor: 'var(--bg-surface)',
            border: '1px solid var(--border-medium)',
            borderRadius: 'var(--radius-card)',
            padding: '12px 14px',
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'space-between',
            boxShadow: 'var(--shadow-tactical)',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
            <span style={{ fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>
              Active Agent Fleet
            </span>
            <Cpu size={14} style={{ color: 'var(--accent-telemetry)' }} />
          </div>
          <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
            <span style={{ fontSize: 22, fontWeight: 700, fontFamily: 'var(--font-mono)', color: 'var(--text-primary)' }} className="tabular-nums">
              {workingAgents.length}
            </span>
            <span style={{ fontSize: 11, color: 'var(--text-secondary)' }}>Working ({idleAgents.length} Idle · {agents.length} Total)</span>
          </div>
          <div style={{ borderTop: '1px solid var(--border-subtle)', marginTop: 10, paddingTop: 8, display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>
            <span>Worktree Isolation</span>
            <span style={{ color: 'var(--accent-green)' }}>Zero Collision Locks</span>
          </div>
        </div>

        {/* Merge Pipeline (4 Cols) */}
        <div
          style={{
            gridColumn: 'span 4',
            backgroundColor: 'var(--bg-surface)',
            border: '1px solid var(--border-medium)',
            borderRadius: 'var(--radius-card)',
            padding: '12px 14px',
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'space-between',
            boxShadow: 'var(--shadow-tactical)',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
            <span style={{ fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>
              Serialized Merge Queue
            </span>
            <GitMerge size={14} style={{ color: 'var(--accent-primary)' }} />
          </div>
          <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
            <span style={{ fontSize: 22, fontWeight: 700, fontFamily: 'var(--font-mono)', color: 'var(--text-primary)' }} className="tabular-nums">
              {mergeQueue.filter((m) => m.status === 'READY').length}
            </span>
            <span style={{ fontSize: 11, color: 'var(--text-secondary)' }}>Queued ({completedTasks.length} Merged into Main)</span>
          </div>
          <div style={{ borderTop: '1px solid var(--border-subtle)', marginTop: 10, paddingTop: 8, display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>
            <span>Integration Strategy</span>
            <span style={{ color: 'var(--text-primary)' }}>Automated CAS Fast-Forward</span>
          </div>
        </div>
      </div>

      {/* Needs Attention Section (If any failed or blocked) */}
      {needsAttention.length > 0 && (
        <div style={{ backgroundColor: 'var(--bg-surface)', border: '1px solid rgba(244, 63, 94, 0.35)', borderRadius: 'var(--radius-card)', overflow: 'hidden', boxShadow: 'var(--shadow-tactical)' }}>
          <div style={{ padding: '8px 14px', backgroundColor: 'rgba(244, 63, 94, 0.08)', borderBottom: '1px solid rgba(244, 63, 94, 0.2)', fontWeight: 600, fontSize: 11, display: 'flex', alignItems: 'center', justifyContent: 'space-between', color: 'var(--accent-red)', fontFamily: 'var(--font-mono)' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <ShieldAlert size={14} /> ATTENTION REQUIRED ({needsAttention.length})
            </div>
            <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>Scope Violations or Check Failures</span>
          </div>
          <div style={{ padding: 8, display: 'flex', flexDirection: 'column', gap: 6 }}>
            {needsAttention.map((t) => (
              <div
                key={t.id}
                style={{ padding: '8px 12px', backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-xs)', display: 'flex', justifyContent: 'space-between', alignItems: 'center', cursor: 'pointer' }}
                onClick={() => onSelectTask(t)}
                title="Click to open task workspace and resolve scope violation or test failure"
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <span className={`badge badge-${t.state}`}>{t.state}</span>
                  <span style={{ fontWeight: 600, fontFamily: 'var(--font-mono)', color: 'var(--text-primary)' }}>{t.id}</span>
                  <span style={{ color: 'var(--text-secondary)' }}>{t.title}</span>
                </div>
                <span style={{ color: 'var(--accent-red)', fontSize: 11, display: 'inline-flex', alignItems: 'center', gap: 4, fontWeight: 600 }}>
                  Resolve Incident <ArrowRight size={11} />
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Primary Split Decks: Active Execution vs Ready & Review */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(12, 1fr)', gap: 14, flex: 1 }}>
        {/* Active Execution Runs (7 Cols) */}
        <div style={{ gridColumn: 'span 7', backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-card)', display: 'flex', flexDirection: 'column', boxShadow: 'var(--shadow-tactical)' }}>
          <div style={{ padding: '10px 14px', borderBottom: '1px solid var(--border-subtle)', fontWeight: 600, fontSize: 11, display: 'flex', alignItems: 'center', justifyContent: 'space-between', fontFamily: 'var(--font-mono)' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
              <Play size={12} style={{ color: 'var(--accent-primary)' }} /> ACTIVE WORKTREE EXECUTION ({runningTasks.length})
            </div>
            <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>Exclusive Leases Active</span>
          </div>
          <div style={{ padding: 10, display: 'flex', flexDirection: 'column', gap: 8, flex: 1, overflowY: 'auto' }}>
            {runningTasks.length === 0 ? (
              <div style={{ color: 'var(--text-muted)', padding: '30px 12px', textAlign: 'center', fontSize: 12 }}>
                No active agent executions currently running.
                <div style={{ marginTop: 10 }}>
                  <button
                    className="btn btn-secondary"
                    style={{ height: 26, fontSize: 11, fontFamily: 'var(--font-mono)' }}
                    onClick={onOpenNewTask}
                    title="Create a new task to dispatch to an AI agent"
                  >
                    + Create Task
                  </button>
                </div>
              </div>
            ) : (
              runningTasks.map((t) => (
                <div
                  key={t.id}
                  style={{
                    padding: 10,
                    backgroundColor: 'var(--bg-card)',
                    border: '1px solid var(--border-medium)',
                    borderRadius: 'var(--radius-xs)',
                    cursor: 'pointer',
                    transition: 'border-color var(--duration-tactical) var(--ease-tactical)',
                  }}
                  onClick={() => onSelectTask(t)}
                  title="Click to view live execution progress, file locks, or steer agent"
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 5 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, color: 'var(--accent-primary)', fontSize: 11 }}>{t.id}</span>
                      <span style={{ fontWeight: 600, color: 'var(--text-primary)' }}>{t.title}</span>
                    </div>
                    <span className="badge badge-RUNNING">{t.substate}</span>
                  </div>
                  <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--text-muted)', fontFamily: 'var(--font-mono)' }}>
                    <span>Agent: {t.assigned_agent_id ? <span style={{ color: 'var(--text-secondary)' }}>{t.assigned_agent_id}</span> : 'Pending Claim'}</span>
                    <span>Branch: {t.branch_name ? <span style={{ color: 'var(--text-secondary)' }}>{t.branch_name.replace('agentxflow/', '')}</span> : 'Allocating...'}</span>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>

        {/* Ready & Review Queue (5 Cols) */}
        <div style={{ gridColumn: 'span 5', backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-card)', display: 'flex', flexDirection: 'column', boxShadow: 'var(--shadow-tactical)' }}>
          <div style={{ padding: '10px 14px', borderBottom: '1px solid var(--border-subtle)', fontWeight: 600, fontSize: 11, display: 'flex', alignItems: 'center', justifyContent: 'space-between', fontFamily: 'var(--font-mono)' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
              <Clock size={12} style={{ color: 'var(--accent-telemetry)' }} /> READY & REVIEW QUEUE ({waitingTasks.length})
            </div>
            <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>Action Required</span>
          </div>
          <div style={{ padding: 10, display: 'flex', flexDirection: 'column', gap: 8, flex: 1, overflowY: 'auto' }}>
            {waitingTasks.length === 0 ? (
              <div style={{ color: 'var(--text-muted)', padding: '30px 12px', textAlign: 'center', fontSize: 12 }}>
                Queue is clear. All tasks have completed or are in backlog.
              </div>
            ) : (
              waitingTasks.map((t) => (
                <div
                  key={t.id}
                  style={{
                    padding: 10,
                    backgroundColor: 'var(--bg-card)',
                    border: '1px solid var(--border-medium)',
                    borderRadius: 'var(--radius-xs)',
                    cursor: 'pointer',
                    transition: 'border-color var(--duration-tactical) var(--ease-tactical)',
                  }}
                  onClick={() => onSelectTask(t)}
                  title="Click to assign an agent or approve verified changes"
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 5 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, color: 'var(--accent-telemetry)', fontSize: 11 }}>{t.id}</span>
                      <span style={{ fontWeight: 600, color: 'var(--text-primary)' }}>{t.title}</span>
                    </div>
                    <span className={`badge badge-${t.state}`}>{t.state}</span>
                  </div>
                  <div style={{ fontSize: 11, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                    <span style={{ color: 'var(--text-muted)', fontFamily: 'var(--font-mono)' }}>Priority: {t.priority}</span>
                    <span
                      style={{
                        display: 'inline-flex',
                        alignItems: 'center',
                        gap: 4,
                        color: t.state === 'READY' ? 'var(--accent-primary)' : 'var(--accent-telemetry)',
                        fontWeight: 600,
                        fontSize: 11,
                      }}
                    >
                      {t.state === 'READY' ? 'Assign Agent' : 'Review & Merge'}
                      <ArrowRight size={11} />
                    </span>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
