import React, { useState } from 'react';
import { Task, TaskDependency } from '../types';
import { List, LayoutGrid, GitFork, Plus, ArrowRight } from 'lucide-react';

interface WorkViewProps {
  tasks: Task[];
  agents?: any[];
  dependencies: TaskDependency[];
  selectedTask: Task | null;
  onSelectTask: (t: Task) => void;
  onOpenNewTaskModal: () => void;
}

const BOARD_COLUMNS = [
  { label: 'BACKLOG', states: ['BACKLOG'], desc: 'Unassigned tasks waiting to be worked on' },
  { label: 'READY', states: ['READY'], desc: 'Prerequisite tasks finished; ready for an agent to claim' },
  { label: 'RUNNING', states: ['RUNNING'], desc: 'Agent actively executing inside isolated Git worktree' },
  { label: 'REVIEW', states: ['REVIEW', 'MERGE_READY'], desc: 'Tests passed; awaiting human review or merge queue' },
  { label: 'DONE', states: ['DONE'], desc: 'Successfully merged into the main branch' },
];

export const WorkView: React.FC<WorkViewProps> = ({
  tasks,
  dependencies,
  selectedTask,
  onSelectTask,
  onOpenNewTaskModal,
}) => {
  const [viewMode, setViewMode] = useState<'list' | 'board' | 'dag'>('list');
  const [filterState, setFilterState] = useState<string>('ALL');

  const filteredTasks = tasks.filter((t) => (filterState === 'ALL' ? true : t.state === filterState));

  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      {/* Work Sub-Header & Controls */}
      <div
        style={{
          height: 42,
          borderBottom: '1px solid var(--border-medium)',
          backgroundColor: 'var(--bg-surface)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '0 14px',
          flexShrink: 0,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          {/* Segmented Physical Switch */}
          <div
            style={{
              display: 'flex',
              backgroundColor: 'var(--bg-input)',
              padding: 2,
              borderRadius: 'var(--radius-xs)',
              border: '1px solid var(--border-medium)',
              gap: 2,
            }}
          >
            <button
              style={{
                height: 24,
                padding: '2px 10px',
                fontSize: 11,
                fontWeight: 600,
                border: 'none',
                cursor: 'pointer',
                display: 'flex',
                alignItems: 'center',
                gap: 5,
                borderRadius: 'var(--radius-xs)',
                backgroundColor: viewMode === 'list' ? 'var(--bg-surface-elevated)' : 'transparent',
                color: viewMode === 'list' ? 'var(--text-primary)' : 'var(--text-muted)',
                boxShadow: viewMode === 'list' ? '0 1px 2px rgba(0, 0, 0, 0.4)' : 'none',
                transition: 'background-color 0.12s cubic-bezier(0.16, 1, 0.3, 1), color 0.12s cubic-bezier(0.16, 1, 0.3, 1)',
              }}
              onClick={() => setViewMode('list')}
              title="Display tasks in a dense, sortable table"
            >
              <List size={12} /> List
            </button>
            <button
              style={{
                height: 24,
                padding: '2px 10px',
                fontSize: 11,
                fontWeight: 600,
                border: 'none',
                cursor: 'pointer',
                display: 'flex',
                alignItems: 'center',
                gap: 5,
                borderRadius: 'var(--radius-xs)',
                backgroundColor: viewMode === 'board' ? 'var(--bg-surface-elevated)' : 'transparent',
                color: viewMode === 'board' ? 'var(--text-primary)' : 'var(--text-muted)',
                boxShadow: viewMode === 'board' ? '0 1px 2px rgba(0, 0, 0, 0.4)' : 'none',
                transition: 'background-color 0.12s cubic-bezier(0.16, 1, 0.3, 1), color 0.12s cubic-bezier(0.16, 1, 0.3, 1)',
              }}
              onClick={() => setViewMode('board')}
              title="Display tasks in Kanban columns (Backlog → Ready → Running → Review → Done)"
            >
              <LayoutGrid size={12} /> Board
            </button>
            <button
              style={{
                height: 24,
                padding: '2px 10px',
                fontSize: 11,
                fontWeight: 600,
                border: 'none',
                cursor: 'pointer',
                display: 'flex',
                alignItems: 'center',
                gap: 5,
                borderRadius: 'var(--radius-xs)',
                backgroundColor: viewMode === 'dag' ? 'var(--bg-surface-elevated)' : 'transparent',
                color: viewMode === 'dag' ? 'var(--text-primary)' : 'var(--text-muted)',
                boxShadow: viewMode === 'dag' ? '0 1px 2px rgba(0, 0, 0, 0.4)' : 'none',
                transition: 'background-color 0.12s cubic-bezier(0.16, 1, 0.3, 1), color 0.12s cubic-bezier(0.16, 1, 0.3, 1)',
              }}
              onClick={() => setViewMode('dag')}
              title="View Task Dependency Graph to see which tasks block each other"
            >
              <GitFork size={12} /> Dependency DAG
            </button>
          </div>

          {/* Filter Dropdown */}
          <select
            className="input-field"
            style={{ width: 130, height: 26, padding: '2px 6px', fontSize: 11, fontFamily: 'var(--font-mono)' }}
            value={filterState}
            onChange={(e) => setFilterState(e.target.value)}
            title="Filter tasks by lifecycle state"
          >
            <option value="ALL">All States ({tasks.length})</option>
            <option value="BACKLOG">BACKLOG</option>
            <option value="READY">READY</option>
            <option value="RUNNING">RUNNING</option>
            <option value="REVIEW">REVIEW</option>
            <option value="DONE">DONE</option>
            <option value="BLOCKED">BLOCKED</option>
          </select>
        </div>

        <button
          className="btn btn-primary"
          style={{ height: 26, fontSize: 11 }}
          onClick={onOpenNewTaskModal}
          title="Create a new engineering task with custom title, prompt, priority, and required test steps"
        >
          <Plus size={13} /> Create Task
        </button>
      </div>

      {/* Main Mode Viewport */}
      <div style={{ flex: 1, overflow: 'auto', display: 'flex' }}>
        {viewMode === 'list' && (
          <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 11, textAlign: 'left', fontVariantNumeric: 'tabular-nums' }}>
            <thead>
              <tr
                style={{
                  borderBottom: '1px solid var(--border-medium)',
                  backgroundColor: 'var(--bg-surface-elevated)',
                  color: 'var(--text-muted)',
                  fontFamily: 'var(--font-mono)',
                  fontSize: 10,
                  letterSpacing: '0.04em',
                }}
              >
                <th style={{ padding: '8px 14px', width: 110 }}>TASK ID</th>
                <th style={{ padding: '8px 14px' }}>TITLE & PROMPT</th>
                <th style={{ padding: '8px 14px', width: 110 }}>STATE</th>
                <th style={{ padding: '8px 14px', width: 90 }}>PRIORITY</th>
                <th style={{ padding: '8px 14px', width: 150 }}>ASSIGNED AGENT</th>
                <th style={{ padding: '8px 14px', width: 180 }}>GIT WORKTREE BRANCH</th>
              </tr>
            </thead>
            <tbody>
              {filteredTasks.length === 0 ? (
                <tr>
                  <td colSpan={6} style={{ padding: 32, textAlign: 'center', color: 'var(--text-muted)' }}>
                    No tasks match the selected state filter.
                  </td>
                </tr>
              ) : (
                filteredTasks.map((t) => {
                  const isSelected = selectedTask?.id === t.id;
                  const isRunning = t.state === 'RUNNING';
                  return (
                    <tr
                      key={t.id}
                      style={{
                        borderBottom: '1px solid var(--border-subtle)',
                        backgroundColor: isSelected ? 'var(--bg-surface-active)' : 'transparent',
                        cursor: 'pointer',
                        transition: 'background-color 0.12s cubic-bezier(0.16, 1, 0.3, 1)',
                      }}
                      onMouseEnter={(e) => {
                        if (!isSelected) e.currentTarget.style.backgroundColor = 'var(--bg-surface-hover)';
                      }}
                      onMouseLeave={(e) => {
                        if (!isSelected) e.currentTarget.style.backgroundColor = 'transparent';
                      }}
                      onClick={() => onSelectTask(t)}
                      title={`Click to open Task Workspace for ${t.id}: ${t.title}`}
                    >
                      <td style={{ padding: '8px 14px', fontFamily: 'var(--font-mono)', fontWeight: 600, color: 'var(--accent-primary)' }}>
                        {t.id}
                      </td>
                      <td style={{ padding: '8px 14px', fontWeight: 500 }}>
                        <div style={{ color: 'var(--text-primary)' }}>{t.title}</div>
                        <div
                          style={{
                            color: 'var(--text-muted)',
                            fontSize: 10,
                            marginTop: 2,
                            maxWidth: 500,
                            overflow: 'hidden',
                            textOverflow: 'ellipsis',
                            whiteSpace: 'nowrap',
                          }}
                        >
                          {t.description}
                        </div>
                      </td>
                      <td style={{ padding: '8px 14px' }}>
                        <span className={`badge badge-${t.state}`} title={`Current status: ${t.state} (${t.substate})`}>
                          {t.state}
                        </span>
                      </td>
                      <td style={{ padding: '8px 14px' }}>
                        <span className={`badge badge-${t.priority}`} title={`Priority level: ${t.priority}`}>
                          {t.priority}
                        </span>
                      </td>
                      <td style={{ padding: '8px 14px', color: 'var(--text-secondary)' }}>
                        {t.assigned_agent_id ? (
                          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                            <span
                              style={{
                                width: 6,
                                height: 6,
                                borderRadius: '50%',
                                backgroundColor: isRunning ? 'var(--accent-amber)' : 'var(--accent-mint)',
                                boxShadow: isRunning ? '0 0 6px var(--accent-amber)' : 'none',
                              }}
                            />
                            <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, color: 'var(--text-primary)' }}>
                              {t.assigned_agent_id}
                            </span>
                          </div>
                        ) : (
                          <span style={{ color: 'var(--text-muted)', fontStyle: 'italic' }}>Unassigned</span>
                        )}
                      </td>
                      <td style={{ padding: '8px 14px', fontFamily: 'var(--font-mono)', color: 'var(--text-muted)', fontSize: 10 }}>
                        {t.branch_name ? (
                          <span style={{ color: 'var(--accent-primary)', backgroundColor: 'var(--bg-input)', padding: '2px 6px', borderRadius: 'var(--radius-xs)', border: '1px solid var(--border-subtle)' }}>
                            {t.branch_name.replace('agentxflow/', '')}
                          </span>
                        ) : (
                          '—'
                        )}
                      </td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        )}

        {viewMode === 'board' && (
          <div style={{ display: 'flex', gap: 12, padding: 14, flex: 1, overflowX: 'auto' }}>
            {BOARD_COLUMNS.map((col) => {
              const colTasks = tasks.filter((t) => col.states.includes(t.state));
              return (
                <div
                  key={col.label}
                  style={{
                    width: 270,
                    minWidth: 270,
                    backgroundColor: 'var(--bg-surface)',
                    border: '1px solid var(--border-medium)',
                    borderRadius: 'var(--radius-card)',
                    display: 'flex',
                    flexDirection: 'column',
                    maxHeight: '100%',
                  }}
                  title={col.desc}
                >
                  <div
                    style={{
                      padding: '8px 12px',
                      borderBottom: '1px solid var(--border-medium)',
                      backgroundColor: 'var(--bg-surface-elevated)',
                      display: 'flex',
                      justifyContent: 'space-between',
                      alignItems: 'center',
                      fontSize: 11,
                      fontFamily: 'var(--font-mono)',
                      fontWeight: 600,
                    }}
                  >
                    <span style={{ color: 'var(--text-secondary)' }}>{col.label}</span>
                    <span
                      style={{
                        color: 'var(--text-primary)',
                        backgroundColor: 'var(--bg-input)',
                        padding: '1px 6px',
                        borderRadius: 'var(--radius-xs)',
                        border: '1px solid var(--border-subtle)',
                        fontSize: 10,
                      }}
                    >
                      {colTasks.length}
                    </span>
                  </div>
                  <div style={{ padding: 8, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 8, flex: 1 }}>
                    {colTasks.map((t) => {
                      const isRunning = t.state === 'RUNNING';
                      return (
                        <div
                          key={t.id}
                          style={{
                            padding: 10,
                            backgroundColor: 'var(--bg-card)',
                            border: isRunning ? '1px solid var(--accent-amber)' : '1px solid var(--border-medium)',
                            borderRadius: 'var(--radius-card)',
                            cursor: 'pointer',
                            transition: 'border-color 0.12s cubic-bezier(0.16, 1, 0.3, 1), box-shadow 0.12s cubic-bezier(0.16, 1, 0.3, 1)',
                            boxShadow: isRunning ? '0 0 10px rgba(245, 166, 35, 0.15)' : 'none',
                          }}
                          onMouseEnter={(e) => {
                            if (!isRunning) e.currentTarget.style.borderColor = 'var(--border-bright)';
                          }}
                          onMouseLeave={(e) => {
                            if (!isRunning) e.currentTarget.style.borderColor = 'var(--border-medium)';
                          }}
                          onClick={() => onSelectTask(t)}
                          title={`Click to view details, assign agent, or run verification for ${t.id}`}
                        >
                          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
                            <span style={{ fontWeight: 700, fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--accent-primary)' }}>
                              {t.id}
                            </span>
                            <span className={`badge badge-${t.priority}`}>{t.priority}</span>
                          </div>
                          <div style={{ fontSize: 11, fontWeight: 500, marginBottom: 8, lineHeight: 1.4, color: 'var(--text-primary)' }}>
                            {t.title}
                          </div>
                          <div style={{ fontSize: 10, color: 'var(--text-muted)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                            <span style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
                              {t.assigned_agent_id ? (
                                <>
                                  <span
                                    style={{
                                      width: 5,
                                      height: 5,
                                      borderRadius: '50%',
                                      backgroundColor: isRunning ? 'var(--accent-amber)' : 'var(--accent-mint)',
                                    }}
                                  />
                                  <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-secondary)' }}>{t.assigned_agent_id}</span>
                                </>
                              ) : (
                                'Unassigned'
                              )}
                            </span>
                            <span style={{ color: 'var(--accent-primary)', display: 'flex', alignItems: 'center', gap: 3, fontWeight: 600 }}>
                              Inspect <ArrowRight size={10} />
                            </span>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {viewMode === 'dag' && (
          <div style={{ padding: 20, flex: 1, overflowY: 'auto' }}>
            <div style={{ marginBottom: 16 }}>
              <h3 style={{ fontSize: 13, fontWeight: 600, fontFamily: 'var(--font-mono)' }}>Task Dependency Graph (DAG)</h3>
              <p style={{ fontSize: 11, color: 'var(--text-secondary)', marginTop: 2 }}>
                Tasks cannot be claimed by agents until all their prerequisite blocker tasks have reached <strong>DONE</strong>.
              </p>
            </div>

            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              {tasks.map((t) => {
                const deps = dependencies.filter((d) => d.task_id === t.id);
                return (
                  <div
                    key={t.id}
                    style={{
                      padding: 14,
                      backgroundColor: 'var(--bg-surface)',
                      border: '1px solid var(--border-medium)',
                      borderRadius: 'var(--radius-card)',
                      cursor: 'pointer',
                      transition: 'border-color 0.12s cubic-bezier(0.16, 1, 0.3, 1)',
                    }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.borderColor = 'var(--border-bright)';
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.borderColor = 'var(--border-medium)';
                    }}
                    onClick={() => onSelectTask(t)}
                    title="Click to view task details"
                  >
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
                      <span style={{ fontWeight: 600, fontFamily: 'var(--font-mono)' }}>
                        <span style={{ color: 'var(--accent-primary)', marginRight: 6 }}>{t.id}:</span>
                        {t.title}
                      </span>
                      <span className={`badge badge-${t.state}`}>{t.state}</span>
                    </div>
                    <div style={{ fontSize: 11, color: 'var(--text-secondary)', display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                      <span style={{ color: 'var(--text-muted)' }}>Prerequisites:</span>{' '}
                      {deps.length === 0 ? (
                        <span style={{ color: 'var(--accent-mint)', fontWeight: 600, fontFamily: 'var(--font-mono)', fontSize: 10 }}>
                          [UNBLOCKED • READY FOR CLAIM]
                        </span>
                      ) : (
                        deps.map((d) => (
                          <span
                            key={d.id}
                            style={{
                              fontFamily: 'var(--font-mono)',
                              fontSize: 10,
                              color: 'var(--accent-amber)',
                              backgroundColor: 'var(--bg-input)',
                              padding: '2px 6px',
                              borderRadius: 'var(--radius-xs)',
                              border: '1px solid var(--border-subtle)',
                            }}
                          >
                            BLOCKED BY: {d.depends_on_task_id}
                          </span>
                        ))
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

