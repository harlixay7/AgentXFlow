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
        return (
          <div key={agent.id} className="agent-badge">
            <div
              className={`status-dot ${
                agent.status === 'WORKING' ? 'active' : agent.status === 'BLOCKED' ? 'blocked' : 'idle'
              }`}
            />
            <span style={{ fontWeight: 600, color: 'var(--text-primary)' }}>{agent.name}</span>
            <span style={{ color: 'var(--text-muted)' }}>
              {currentTask ? `${currentTask.title.substring(0, 24)}... · ${currentTask.state}` : 'Idle'}
            </span>
          </div>
        );
      })}
    </div>
  );
};
