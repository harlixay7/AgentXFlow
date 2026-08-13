import React from 'react';
import { Project } from '../types';
import { LayoutGrid, Cpu, Activity, Plug, Settings, FolderPlus, Terminal } from 'lucide-react';

interface TopNavProps {
  currentTab: string;
  setCurrentTab: (tab: string) => void;
  projects: Project[];
  selectedProject: Project | null;
  setSelectedProject: (p: Project) => void;
  onOpenNewProjectModal: () => void;
}

export const TopNav: React.FC<TopNavProps> = ({
  currentTab,
  setCurrentTab,
  projects,
  selectedProject,
  setSelectedProject,
  onOpenNewProjectModal,
}) => {
  return (
    <header className="top-nav">
      <div className="brand" title="AgentXFlow Engineering Coordinator">
        <div className="brand-icon">
          <Terminal size={12} />
        </div>
        <span>AgentXFlow</span>
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        {/* Project Selector */}
        <select
          className="input-field"
          style={{ width: 180, padding: '4px 8px', fontSize: 11, fontFamily: 'var(--font-mono)' }}
          value={selectedProject?.id || ''}
          onChange={(e) => {
            const p = projects.find((x) => x.id === e.target.value);
            if (p) setSelectedProject(p);
          }}
          title="Select active repository workspace"
        >
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>

        <button
          className="btn btn-secondary"
          style={{ padding: '4px 10px', fontSize: 11 }}
          onClick={onOpenNewProjectModal}
          title="Import or create a Git repository folder directly from disk"
        >
          <FolderPlus size={13} />
          Import Repo
        </button>
      </div>

      <nav className="nav-links">
        <div
          className={`nav-item ${currentTab === 'tasks' ? 'active' : ''}`}
          onClick={() => setCurrentTab('tasks')}
          title="Task Kanban board and execution pipeline"
        >
          <LayoutGrid size={13} />
          Tasks
        </div>

        <div
          className={`nav-item ${currentTab === 'agents' ? 'active' : ''}`}
          onClick={() => setCurrentTab('agents')}
          title="Registered AI coding agents"
        >
          <Cpu size={13} />
          Agents
        </div>

        <div
          className={`nav-item ${currentTab === 'activity' ? 'active' : ''}`}
          onClick={() => setCurrentTab('activity')}
          title="Audit log timeline stream"
        >
          <Activity size={13} />
          Activity
        </div>

        <div
          className={`nav-item ${currentTab === 'integrations' ? 'active' : ''}`}
          onClick={() => setCurrentTab('integrations')}
          title="MCP server connection setup"
        >
          <Plug size={13} />
          Integrations
        </div>

        <div
          className={`nav-item ${currentTab === 'settings' ? 'active' : ''}`}
          onClick={() => setCurrentTab('settings')}
          title="Master project spec and rules"
        >
          <Settings size={13} />
          Settings
        </div>
      </nav>
    </header>
  );
};
