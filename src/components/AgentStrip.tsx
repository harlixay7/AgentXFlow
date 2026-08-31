import React from 'react';
import { Agent, Task } from '../types';
import { Bot } from 'lucide-react';

interface AgentStripProps {
  agents: Agent[];
  tasks: Task[];
}

export const AgentStrip: React.FC<AgentStripProps> = ({ agents, tasks }) => {
  return (
    <div className="agent-strip">
      <div className="agent-strip-title">
        <Bot size={13} />
        Active Agents
      </div>

      {agents.map((agent) => {
        const currentTask = tasks.find((t) => t.assigned_agent_id === agent.id && t.state !== 'DONE');
        const isWorking = agent.status === 'WORKING';
        const isDisconnected = agent.status === 'DISCONNECTED';
        const taskTitle = agent.active_task_title || (currentTask ? currentTask.title : null);

        return (
          <div key={agent.id} className="agent-badge">
            <div
              className={`status-dot ${
                isWorking ? 'active' : isDisconnected ? 'idle' : agent.status === 'BLOCKED' ? 'blocked' : 'idle'
              }`}
              style={{
                backgroundColor: isWorking
                  ? 'var(--accent-blue)'
                  : isDisconnected
                  ? 'var(--text-muted)'
                  : 'var(--accent-green)',
              }}
            />
            <span style={{ fontWeight: 600, color: 'var(--text-primary)' }}>{agent.name}</span>
            <span style={{ color: 'var(--text-muted)' }}>
              {taskTitle
                ? `${taskTitle.substring(0, 20)}...`
                : isDisconnected
                ? 'Disconnected'
                : 'Idle'}
            </span>
          </div>
        );
      })}
    </div>
  );
};
