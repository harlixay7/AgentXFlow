import React from 'react';
import { Task, Agent, MergeQueueItem } from '../types';
import { Play, Clock, GitMerge, CheckCircle, ShieldAlert, Cpu, BookOpen, Plus, Sparkles } from 'lucide-react';

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

  return (
    <div style={{ flex: 1, padding: 18, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 16 }}>
      {/* 5-Step Pipeline Guide Banner */}
      <div
        style={{
          backgroundColor: 'var(--bg-surface)',
          border: '1px solid var(--border-medium)',
          borderRadius: 'var(--radius-md)',
          padding: '12px 16px',
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: 12,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div
            style={{
              width: 32,
              height: 32,
              borderRadius: 'var(--radius-sm)',
              backgroundColor: 'rgba(88, 166, 255, 0.12)',
              color: 'var(--accent-blue)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
            }}
          >
            <BookOpen size={16} />
          </div>
          <div>
            <div style={{ fontWeight: 600, fontSize: 12, color: 'var(--text-primary)' }}>
              Cross-Agent Pipeline: 1. Connect Repo → 2. Connect Agent → 3. Assign Task & Scope → 4. Verify → 5. Merge
            </div>
            <div style={{ fontSize: 11, color: 'var(--text-secondary)' }}>
              Agents work in isolated Git worktrees and cannot overwrite each other or break the main branch.
            </div>
          </div>
        </div>

        <div style={{ display: 'flex', gap: 8, flexShrink: 0 }}>
          {onNavigateTab && (
            <button
              className="btn btn-secondary"
              style={{ height: 26, fontSize: 11 }}
              onClick={() => onNavigateTab('masterplan')}
              title="Open Masterplan Execution Hub to decompose raw specs into agent chunks"
            >
              <Sparkles size={12} style={{ color: 'var(--accent-blue)' }} /> Masterplan Hub
            </button>
          )}
          <button
            className="btn btn-secondary"
            style={{ height: 26, fontSize: 11 }}
            onClick={onOpenGuide}
            title="Read the full plain-English walkthrough with diagram and examples"
          >
            How It Works
          </button>
          <button
            className="btn btn-primary"
            style={{ height: 26, fontSize: 11 }}
            onClick={onOpenNewTask}
            title="Create a new task with custom prompt and verification steps"
          >
            <Plus size={12} /> New Task
          </button>
        </div>
      </div>

      {/* System Health Indicators */}
      <div
        style={{
          display: 'flex',
          gap: 12,
          alignItems: 'center',
          backgroundColor: 'var(--bg-surface)',
          padding: '8px 14px',
          borderRadius: 'var(--radius-md)',
          border: '1px solid var(--border-subtle)',
          fontSize: 11,
          fontFamily: 'var(--font-mono)',
          flexWrap: 'wrap',
        }}
      >
        <span
          style={{ color: 'var(--accent-green)', display: 'flex', alignItems: 'center', gap: 4 }}
          title="Coordinator Engine is running and monitoring Git worktrees, state gates, and tests"
        >
          <CheckCircle size={13} /> Coordinator Engine: ONLINE
        </span>
        <span style={{ color: 'var(--border-medium)' }}>|</span>
        <span
          style={{ color: 'var(--text-primary)', display: 'flex', alignItems: 'center', gap: 4 }}
          title="Total AI coding agents registered in SQLite database (Codex, Claude, OpenCode, Antigravity)"
        >
          <Cpu size={13} /> {agents.length} Registered Agents
        </span>
        <span style={{ color: 'var(--border-medium)' }}>|</span>
        <span
          style={{ color: 'var(--text-primary)', display: 'flex', alignItems: 'center', gap: 4 }}
          title="Tasks ready to be safely integrated into main branch"
        >
          <GitMerge size={13} /> {mergeQueue.filter((m) => m.status === 'READY').length} In Merge Queue
        </span>
        <span style={{ color: 'var(--border-medium)' }}>|</span>
        <span
          style={{ color: 'var(--accent-blue)' }}
          title="Local Model Context Protocol gateway endpoint accepting JSON-RPC tool calls"
        >
          MCP: 127.0.0.1:7890 (Streamable HTTP)
        </span>
      </div>

      {/* Needs Attention Section */}
      {needsAttention.length > 0 && (
        <div style={{ backgroundColor: 'var(--bg-surface)', border: '1px solid rgba(248, 81, 73, 0.3)', borderRadius: 'var(--radius-md)', overflow: 'hidden' }}>
          <div style={{ padding: '8px 12px', backgroundColor: 'rgba(248, 81, 73, 0.08)', borderBottom: '1px solid rgba(248, 81, 73, 0.2)', fontWeight: 600, fontSize: 12, display: 'flex', alignItems: 'center', gap: 6, color: 'var(--accent-red)' }}>
            <ShieldAlert size={14} /> NEEDS ATTENTION ({needsAttention.length})
          </div>
          <div style={{ padding: 8, display: 'flex', flexDirection: 'column', gap: 6 }}>
            {needsAttention.map((t) => (
              <div
                key={t.id}
                style={{ padding: '8px 12px', backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-sm)', display: 'flex', justifyContent: 'space-between', alignItems: 'center', cursor: 'pointer' }}
                onClick={() => onSelectTask(t)}
                title="Click to open task workspace and resolve scope violation or test failure"
              >
                <div>
                  <span className={`badge badge-${t.state}`} style={{ marginRight: 8 }}>{t.state}</span>
                  <span style={{ fontWeight: 600, fontFamily: 'var(--font-mono)' }}>{t.id}: </span>
                  <span>{t.title}</span>
                </div>
                <span style={{ color: 'var(--text-muted)', fontSize: 11 }}>Click to resolve →</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Next Step Action Cards Grid */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16 }}>
        {/* Running Tasks */}
        <div style={{ backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-md)', display: 'flex', flexDirection: 'column' }}>
          <div style={{ padding: '10px 14px', borderBottom: '1px solid var(--border-subtle)', fontWeight: 600, fontSize: 12, display: 'flex', alignItems: 'center', justifyContent: 'space-between', fontFamily: 'var(--font-mono)' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <Play size={13} style={{ color: 'var(--accent-yellow)' }} /> ACTIVE EXECUTION RUNS ({runningTasks.length})
            </div>
            <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>Executing in Worktrees</span>
          </div>
          <div style={{ padding: 10, display: 'flex', flexDirection: 'column', gap: 8, flex: 1 }}>
            {runningTasks.length === 0 ? (
              <div style={{ color: 'var(--text-muted)', padding: '20px 12px', textAlign: 'center', fontSize: 11 }}>
                No active tasks currently executing.
                <div style={{ marginTop: 8 }}>
                  <button
                    className="btn btn-secondary"
                    style={{ height: 24, fontSize: 11 }}
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
                  style={{ padding: 10, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', cursor: 'pointer' }}
                  onClick={() => onSelectTask(t)}
                  title="Click to view live execution progress, file locks, or steer agent"
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
                    <span style={{ fontWeight: 600 }}>{t.id}: {t.title}</span>
                    <span className="badge badge-RUNNING">{t.substate}</span>
                  </div>
                  <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>
                    <span>Agent: {t.assigned_agent_id || 'Unassigned'}</span>
                    <span>{t.branch_name || 'No branch'}</span>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>

        {/* Ready & Review Queue */}
        <div style={{ backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-medium)', borderRadius: 'var(--radius-md)', display: 'flex', flexDirection: 'column' }}>
          <div style={{ padding: '10px 14px', borderBottom: '1px solid var(--border-subtle)', fontWeight: 600, fontSize: 12, display: 'flex', alignItems: 'center', justifyContent: 'space-between', fontFamily: 'var(--font-mono)' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <Clock size={13} style={{ color: 'var(--accent-blue)' }} /> READY & REVIEW QUEUE ({waitingTasks.length})
            </div>
            <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>Awaiting Action</span>
          </div>
          <div style={{ padding: 10, display: 'flex', flexDirection: 'column', gap: 8, flex: 1 }}>
            {waitingTasks.length === 0 ? (
              <div style={{ color: 'var(--text-muted)', padding: '20px 12px', textAlign: 'center', fontSize: 11 }}>
                Queue is clear. All tasks have completed or are in backlog.
              </div>
            ) : (
              waitingTasks.map((t) => (
                <div
                  key={t.id}
                  style={{ padding: 10, backgroundColor: 'var(--bg-card)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', cursor: 'pointer' }}
                  onClick={() => onSelectTask(t)}
                  title="Click to assign an agent or approve verified changes"
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
                    <span style={{ fontWeight: 600 }}>{t.id}: {t.title}</span>
                    <span className={`badge badge-${t.state}`}>{t.state}</span>
                  </div>
                  <div style={{ fontSize: 11, color: 'var(--text-muted)', display: 'flex', justifyContent: 'space-between' }}>
                    <span>Priority: {t.priority}</span>
                    <span style={{ color: 'var(--accent-blue)' }}>
                      {t.state === 'READY' ? '👉 Assign Agent' : '👉 Review & Merge'}
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
