import React, { useState } from 'react';
import { Task, TaskDependency } from '../types';
import { List, LayoutGrid, GitFork, Plus } from 'lucide-react';

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
      <div style={{ height: 40, borderBottom: '1px solid var(--border-medium)', backgroundColor: 'var(--bg-surface)', display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '0 14px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          {/* View Toggle */}
          <div style={{ display: 'flex', backgroundColor: 'var(--bg-input)', padding: 2, borderRadius: 'var(--radius-sm)', border: '1px solid var(--border-subtle)' }}>
            <button
              className={`btn btn-secondary ${viewMode === 'list' ? 'active' : ''}`}
              style={{ height: 24, padding: '2px 8px', fontSize: 11, border: 'none' }}
              onClick={() => setViewMode('list')}
              title="Display tasks in a dense, sortable table"
            >
              <List size={12} /> List
            </button>
            <button
              className={`btn btn-secondary ${viewMode === 'board' ? 'active' : ''}`}
              style={{ height: 24, padding: '2px 8px', fontSize: 11, border: 'none' }}
              onClick={() => setViewMode('board')}
              title="Display tasks in Kanban columns (Backlog → Ready → Running → Review → Done)"
            >
              <LayoutGrid size={12} /> Board
            </button>
            <button
              className={`btn btn-secondary ${viewMode === 'dag' ? 'active' : ''}`}
              style={{ height: 24, padding: '2px 8px', fontSize: 11, border: 'none' }}
              onClick={() => setViewMode('dag')}
              title="View Task Dependency Graph to see which tasks block each other"
            >
              <GitFork size={12} /> Dependency DAG
            </button>
          </div>

          {/* Filter Dropdown */}
          <select
            className="input-field"
            style={{ width: 120, height: 26, padding: '2px 6px', fontSize: 11, fontFamily: 'var(--font-mono)' }}
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
          <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 11, textAlign: 'left' }}>
            <thead>
              <tr style={{ borderBottom: '1px solid var(--border-medium)', backgroundColor: 'var(--bg-input)', color: 'var(--text-muted)', fontFamily: 'var(--font-mono)' }}>
                <th style={{ padding: '8px 12px', width: 100 }}>TASK ID</th>
                <th style={{ padding: '8px 12px' }}>TITLE & PROMPT</th>
                <th style={{ padding: '8px 12px', width: 110 }}>STATE</th>
                <th style={{ padding: '8px 12px', width: 90 }}>PRIORITY</th>
                <th style={{ padding: '8px 12px', width: 140 }}>ASSIGNED AGENT</th>
                <th style={{ padding: '8px 12px', width: 170 }}>GIT WORKTREE BRANCH</th>
              </tr>
            </thead>
            <tbody>
              {filteredTasks.map((t) => {
                const isSelected = selectedTask?.id === t.id;
                return (
                  <tr
                    key={t.id}
                    style={{
                      borderBottom: '1px solid var(--border-subtle)',
                      backgroundColor: isSelected ? 'var(--bg-surface-active)' : 'transparent',
                      cursor: 'pointer',
                    }}
                    onClick={() => onSelectTask(t)}
                    title={`Click to open Task Workspace for ${t.id}: ${t.title}`}
                  >
                    <td style={{ padding: '8px 12px', fontFamily: 'var(--font-mono)', fontWeight: 600, color: 'var(--accent-blue)' }}>{t.id}</td>
                    <td style={{ padding: '8px 12px', fontWeight: 500 }}>
                      <div>{t.title}</div>
                      <div style={{ color: 'var(--text-muted)', fontSize: 10, marginTop: 2, maxWidth: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {t.description}
                      </div>
                    </td>
                    <td style={{ padding: '8px 12px' }}>
                      <span className={`badge badge-${t.state}`} title={`Current status: ${t.state} (${t.substate})`}>
                        {t.state}
                      </span>
                    </td>
                    <td style={{ padding: '8px 12px' }}>
                      <span className={`badge badge-${t.priority}`} title={`Priority level: ${t.priority}`}>
                        {t.priority}
                      </span>
                    </td>
                    <td style={{ padding: '8px 12px', color: 'var(--text-secondary)' }}>
                      {t.assigned_agent_id ? (
                        <span style={{ color: 'var(--accent-blue)', fontWeight: 600 }}>● {t.assigned_agent_id}</span>
                      ) : (
                        <span style={{ color: 'var(--text-muted)' }}>Unassigned (Click to claim)</span>
                      )}
                    </td>
                    <td style={{ padding: '8px 12px', fontFamily: 'var(--font-mono)', color: 'var(--text-muted)' }}>
                      {t.branch_name ? t.branch_name.replace('agentxflow/', '') : '—'}
                    </td>
                  </tr>
                );
              })}
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
                    width: 260,
                    minWidth: 260,
                    backgroundColor: 'var(--bg-surface)',
                    border: '1px solid var(--border-subtle)',
                    borderRadius: 'var(--radius-md)',
                    display: 'flex',
                    flexDirection: 'column',
                    maxHeight: '100%',
                  }}
                  title={col.desc}
                >
                  <div style={{ padding: '8px 10px', borderBottom: '1px solid var(--border-subtle)', display: 'flex', justifyContent: 'space-between', alignItems: 'center', fontSize: 11, fontFamily: 'var(--font-mono)', fontWeight: 600 }}>
                    <span>{col.label}</span>
                    <span style={{ color: 'var(--text-muted)', backgroundColor: 'var(--bg-input)', padding: '1px 6px', borderRadius: 10 }}>
                      {colTasks.length}
                    </span>
                  </div>
                  <div style={{ padding: 8, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 8, flex: 1 }}>
                    {colTasks.map((t) => (
                      <div
                        key={t.id}
                        style={{
                          padding: 10,
                          backgroundColor: 'var(--bg-card)',
                          border: '1px solid var(--border-medium)',
                          borderRadius: 'var(--radius-sm)',
                          cursor: 'pointer',
                          transition: 'border-color 0.12s',
                        }}
                        onClick={() => onSelectTask(t)}
                        title={`Click to view details, assign agent, or run verification for ${t.id}`}
                      >
                        <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
                          <span style={{ fontWeight: 700, fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--accent-blue)' }}>{t.id}</span>
                          <span className={`badge badge-${t.priority}`}>{t.priority}</span>
                        </div>
                        <div style={{ fontSize: 11, fontWeight: 500, marginBottom: 8, lineHeight: 1.4 }}>{t.title}</div>
                        <div style={{ fontSize: 10, color: 'var(--text-muted)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                          <span>{t.assigned_agent_id ? `● ${t.assigned_agent_id}` : 'Unassigned'}</span>
                          <span style={{ color: 'var(--accent-blue)' }}>Details →</span>
                        </div>
                      </div>
                    ))}
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
                      borderRadius: 'var(--radius-md)',
                      cursor: 'pointer',
                    }}
                    onClick={() => onSelectTask(t)}
                    title="Click to view task details"
                  >
                    <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
                      <span style={{ fontWeight: 600, fontFamily: 'var(--font-mono)' }}>{t.id}: {t.title}</span>
                      <span className={`badge badge-${t.state}`}>{t.state}</span>
                    </div>
                    <div style={{ fontSize: 11, color: 'var(--text-secondary)' }}>
                      Blocked By:{' '}
                      {deps.length === 0 ? (
                        <span style={{ color: 'var(--accent-green)', fontWeight: 600 }}>No prerequisites (Ready for agent execution)</span>
                      ) : (
                        deps.map((d) => (
                          <span key={d.id} style={{ fontFamily: 'var(--font-mono)', color: 'var(--accent-yellow)', marginRight: 6 }}>
                            [Prerequisite Task: {d.depends_on_task_id}]
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
