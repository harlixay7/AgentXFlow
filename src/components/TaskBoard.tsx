import React from 'react';
import { Task, Agent, TaskState } from '../types';
import { Plus, GitBranch } from 'lucide-react';

interface TaskBoardProps {
  tasks: Task[];
  agents: Agent[];
  selectedTask: Task | null;
  onSelectTask: (t: Task) => void;
  onOpenNewTaskModal: () => void;
}

const COLUMNS: { label: string; states: TaskState[]; desc: string }[] = [
  { label: 'BACKLOG', states: ['BACKLOG'], desc: 'Unassigned tasks pending sprint readiness' },
  { label: 'READY', states: ['READY'], desc: 'Tasks ready for AI agent assignment & worktree allocation' },
  { label: 'ACTIVE', states: ['RUNNING'], desc: 'Tasks currently claimed with active scope lock leases' },
  { label: 'REVIEW', states: ['REVIEW', 'MERGE_READY'], desc: 'Tasks verified by Coordinator awaiting merge approval' },
  { label: 'DONE', states: ['DONE'], desc: 'Safely integrated tasks merged into main branch' },
];

export const TaskBoard: React.FC<TaskBoardProps> = ({
  tasks,
  agents,
  selectedTask,
  onSelectTask,
  onOpenNewTaskModal,
}) => {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, overflow: 'hidden' }}>
      {/* Board Actions Header */}
      <div
        style={{
          padding: '10px 18px',
          borderBottom: '1px solid var(--border-subtle)',
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          backgroundColor: 'var(--bg-surface)',
        }}
      >
        <div>
          <div style={{ fontWeight: 700, fontSize: 13, color: 'var(--text-primary)' }}>
            Engineering Task Execution Pipeline
          </div>
          <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>
            Tasks move strictly through state gates. Click any task to manage scope locks, steps, or submission.
          </div>
        </div>

        <button
          className="btn btn-primary"
          onClick={onOpenNewTaskModal}
          title="Create a new task with custom prompt, priority, required steps, and acceptance criteria"
        >
          <Plus size={14} /> Create Task
        </button>
      </div>

      <div className="kanban-board">
        {COLUMNS.map((col) => {
          const colTasks = tasks.filter((t) => col.states.includes(t.state));

          return (
            <div key={col.label} className="kanban-column" title={col.desc}>
              <div className="column-header">
                <span>{col.label}</span>
                <span className="column-count">{colTasks.length}</span>
              </div>

              <div className="column-cards">
                {colTasks.map((task) => {
                  const assignedAgent = agents.find((a) => a.id === task.assigned_agent_id);
                  const isSelected = selectedTask?.id === task.id;

                  return (
                    <div
                      key={task.id}
                      className="task-card"
                      style={{
                        borderColor: isSelected ? 'var(--accent-blue)' : 'var(--border-subtle)',
                        backgroundColor: isSelected ? 'var(--bg-surface-active)' : 'var(--bg-card)',
                      }}
                      onClick={() => onSelectTask(task)}
                      title={`Click to open detail panel for ${task.title}`}
                    >
                      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                        <span className={`badge badge-${task.priority}`} title={`Task Priority: ${task.priority}`}>
                          {task.priority}
                        </span>
                        <span style={{ fontSize: 10, color: 'var(--text-muted)', fontFamily: 'var(--font-mono)' }} title={`Current State: ${task.state}`}>
                          {task.state}
                        </span>
                      </div>

                      <div className="task-card-title" style={{ marginTop: 6, fontWeight: 600 }}>{task.title}</div>

                      <div className="task-card-meta" style={{ marginTop: 8, display: 'flex', justifyContent: 'space-between', fontSize: 10 }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 4 }} title={assignedAgent ? `Assigned Agent: ${assignedAgent.name}` : 'No agent assigned yet'}>
                          {assignedAgent ? (
                            <span style={{ color: 'var(--accent-blue)', fontWeight: 600 }}>
                              ● {assignedAgent.name}
                            </span>
                          ) : (
                            <span style={{ color: 'var(--text-muted)' }}>Unassigned</span>
                          )}
                        </div>

                        {task.branch_name && (
                          <div style={{ display: 'flex', alignItems: 'center', gap: 3, color: 'var(--text-muted)' }} title={`Git Worktree Branch: ${task.branch_name}`}>
                            <GitBranch size={10} />
                            {task.branch_name.replace('agentxflow/', '')}
                          </div>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
};
