import React, { useState } from 'react';
import { Project, Task, Agent, MergeQueueItem, EventItem, TaskDependency } from '../types';
import {
  Activity,
  LayoutGrid,
  Cpu,
  GitMerge,
  Plug,
  FolderPlus,
  Terminal,
  Search,
  CheckCircle,
  HelpCircle,
  BookOpen,
  Sparkles,
} from 'lucide-react';
import { MissionControlView } from './MissionControlView';
import { WorkView } from './WorkView';
import { MasterplanHubView } from './MasterplanHubView';
import { TaskWorkspace } from './TaskWorkspace';
import { ReviewCenter } from './ReviewCenter';
import { MergeQueueView } from './MergeQueueView';
import { AgentManagementView } from './AgentManagementView';
import { IntegrationsView } from './IntegrationsView';
import { BottomPanel } from './BottomPanel';
import { CommandPalette } from './CommandPalette';
import { RepoImportWizard } from './RepoImportWizard';
import { NewTaskModal } from './NewTaskModal';
import { WorkflowGuideModal } from './WorkflowGuideModal';

interface WorkbenchShellProps {
  projects: Project[];
  activeProject: Project | null;
  tasks: Task[];
  agents: Agent[];
  mergeQueue: MergeQueueItem[];
  events: EventItem[];
  dependencies: TaskDependency[];
  selectedTask: Task | null;
  onSelectProject: (p: Project) => void;
  onSelectTask: (t: Task | null) => void;
  onRefresh: () => void;
}

export const WorkbenchShell: React.FC<WorkbenchShellProps> = ({
  projects,
  activeProject,
  tasks,
  agents,
  mergeQueue,
  events,
  dependencies,
  selectedTask,
  onSelectProject,
  onSelectTask,
  onRefresh,
}) => {
  const [activeTab, setActiveTab] = useState<'overview' | 'masterplan' | 'work' | 'agents' | 'review' | 'merge_queue' | 'integrations'>('overview');
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [showImportWizard, setShowImportWizard] = useState(false);
  const [showNewTaskModal, setShowNewTaskModal] = useState(false);
  const [showGuideModal, setShowGuideModal] = useState(false);

  return (
    <div className="workbench-shell">
      {/* Top Command Header */}
      <header className="top-command-header">
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div className="brand-section" title="AgentXFlow by Viducia — Cross-Agent Engineering Coordinator by harlixay7" style={{ gap: 8 }}>
            <Terminal size={14} style={{ color: 'var(--accent-blue)' }} />
            <div style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.15 }}>
              <span style={{ fontWeight: 700, letterSpacing: '0.04em', fontSize: 12, color: 'var(--text-primary)' }}>AGENTXFLOW</span>
              <span style={{ fontSize: 9, color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>by Viducia</span>
            </div>
          </div>

          {/* Project Switcher */}
          <select
            className="input-field"
            style={{ width: 180, height: 26, fontSize: 11, fontFamily: 'var(--font-mono)' }}
            value={activeProject?.id || ''}
            onChange={(e) => {
              const p = projects.find((x) => x.id === e.target.value);
              if (p) onSelectProject(p);
            }}
            title="Active project repository. Click to switch between connected repositories."
          >
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>

          <button
            className="btn btn-secondary"
            style={{ height: 26, fontSize: 11 }}
            onClick={() => setShowImportWizard(true)}
            title="Import an existing codebase or initialize a new Git repo from disk"
          >
            <FolderPlus size={12} /> Import Repo
          </button>
        </div>

        {/* Global Command Palette Trigger */}
        <div
          className="command-bar-trigger"
          onClick={() => setIsCommandPaletteOpen(true)}
          title="Press Ctrl+K or Cmd+K to open global search & command launcher"
        >
          <Search size={12} />
          <span>Type a command or search actions...</span>
          <span className="kbd-shortcut" style={{ marginLeft: 'auto' }}>Ctrl+K</span>
        </div>

        {/* Right Action Group */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          {/* Plain English Guide Button */}
          <button
            className="btn btn-secondary"
            style={{ height: 26, fontSize: 11, color: 'var(--accent-blue)', borderColor: 'rgba(88, 166, 255, 0.4)' }}
            onClick={() => setShowGuideModal(true)}
            title="Open the step-by-step visual guide explaining how AgentXFlow coordinates agents"
          >
            <BookOpen size={12} /> How to Use
          </button>

          {/* Live Active Agent Counter */}
          <div
            style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, fontFamily: 'var(--font-mono)', padding: '2px 8px', backgroundColor: 'var(--bg-input)', borderRadius: 'var(--radius-sm)', border: '1px solid var(--border-subtle)' }}
            title="Active agent connections communicating with internal MCP 2026 gateway (127.0.0.1:7890)"
          >
            <span style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: agents.some((a) => a.status === 'WORKING') ? 'var(--accent-yellow)' : 'var(--accent-green)' }} />
            <span>{agents.filter((a) => a.status === 'WORKING').length}/{agents.length} Agents</span>
          </div>
        </div>
      </header>

      {/* Main Body */}
      <div className="workbench-body">
        {/* Left Nav Sidebar */}
        <nav className="left-nav-sidebar">
          <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            <div
              className={`nav-item ${activeTab === 'overview' ? 'active' : ''}`}
              onClick={() => setActiveTab('overview')}
              title="Overview dashboard: system health, running agent tasks, and high-priority alerts"
            >
              <Activity size={13} /> Overview
            </div>
            <div
              className={`nav-item ${activeTab === 'masterplan' ? 'active' : ''}`}
              onClick={() => setActiveTab('masterplan')}
              title="Masterplan Hub: Ingest arbitrary plans, auto-decompose into structured checklists, and coordinate chunked execution"
            >
              <Sparkles size={13} style={{ color: activeTab === 'masterplan' ? 'var(--accent-blue)' : undefined }} /> Masterplan Hub
            </div>
            <div
              className={`nav-item ${activeTab === 'work' ? 'active' : ''}`}
              onClick={() => setActiveTab('work')}
              title="Work view: List, Board, and DAG view of all engineering tasks"
            >
              <LayoutGrid size={13} /> Work (Tasks)
            </div>
            <div
              className={`nav-item ${activeTab === 'agents' ? 'active' : ''}`}
              onClick={() => setActiveTab('agents')}
              title="Registered AI Agents, profiles, and ACP capability negotiations"
            >
              <Cpu size={13} /> Agents & ACP
            </div>
            <div
              className={`nav-item ${activeTab === 'review' ? 'active' : ''}`}
              onClick={() => setActiveTab('review')}
              title="Review center: inspect verified agent code diffs and SHA-256 Proof Bundles"
            >
              <CheckCircle size={13} /> Review Center
            </div>
            <div
              className={`nav-item ${activeTab === 'merge_queue' ? 'active' : ''}`}
              onClick={() => setActiveTab('merge_queue')}
              title="Serialized merge queue: background integration worktree merges into main"
            >
              <GitMerge size={13} /> Merge Queue
            </div>
            <div
              className={`nav-item ${activeTab === 'integrations' ? 'active' : ''}`}
              onClick={() => setActiveTab('integrations')}
              title="Model Context Protocol (MCP 2026-07-28) setup instructions and 1-click config"
            >
              <Plug size={13} /> MCP Gateway
            </div>
          </div>

          {/* Quick Help Link at Bottom of Sidebar */}
          <div
            style={{
              padding: '10px 8px',
              borderTop: '1px solid var(--border-subtle)',
              display: 'flex',
              flexDirection: 'column',
              gap: 6,
            }}
          >
            <div
              className="nav-item"
              style={{ fontSize: 11, color: 'var(--text-muted)', padding: '4px 6px' }}
              onClick={() => setShowGuideModal(true)}
              title="Learn how the 5-step cross-agent coordination workflow operates"
            >
              <HelpCircle size={13} /> Workflow Guide
            </div>
          </div>
        </nav>

        {/* Main Viewport */}
        <div className="main-viewport-container">
          <div className="primary-view-area">
            {activeTab === 'overview' && (
              <MissionControlView
                tasks={tasks}
                agents={agents}
                mergeQueue={mergeQueue}
                onSelectTask={(t) => onSelectTask(t)}
                onOpenGuide={() => setShowGuideModal(true)}
                onOpenNewTask={() => setShowNewTaskModal(true)}
                onOpenImport={() => setShowImportWizard(true)}
                onNavigateTab={(t: any) => setActiveTab(t)}
              />
            )}

            {activeTab === 'masterplan' && (
              <MasterplanHubView
                projectId={activeProject?.id || ''}
                agents={agents}
                onRefreshTasks={onRefresh}
              />
            )}

            {activeTab === 'work' && (
              <WorkView
                tasks={tasks}
                agents={agents}
                dependencies={dependencies}
                selectedTask={selectedTask}
                onSelectTask={(t) => onSelectTask(t)}
                onOpenNewTaskModal={() => setShowNewTaskModal(true)}
              />
            )}

            {activeTab === 'agents' && <AgentManagementView agents={agents} onRefresh={onRefresh} />}

            {activeTab === 'review' && <ReviewCenter tasks={tasks} onRefresh={onRefresh} />}

            {activeTab === 'merge_queue' && (
              <MergeQueueView
                queue={mergeQueue}
                projectId={activeProject?.id || ''}
                onRefresh={onRefresh}
              />
            )}

            {activeTab === 'integrations' && (
              <IntegrationsView agents={agents} onRefreshAgents={onRefresh} />
            )}

            {/* Task Workspace Slide-Over Drawer */}
            {selectedTask && (
              <TaskWorkspace
                task={selectedTask}
                agents={agents}
                steps={[]}
                criteria={[]}
                leases={[]}
                onClose={() => onSelectTask(null)}
                onRefresh={onRefresh}
              />
            )}
          </div>

          {/* Bottom Debugger Panel */}
          <BottomPanel events={events} />
        </div>
      </div>

      {/* Global Modals */}
      <CommandPalette
        isOpen={isCommandPaletteOpen}
        onClose={() => setIsCommandPaletteOpen(false)}
        onNavigateTab={(t: any) => setActiveTab(t)}
        onOpenNewTaskModal={() => setShowNewTaskModal(true)}
        onOpenImportModal={() => setShowImportWizard(true)}
      />

      <WorkflowGuideModal
        isOpen={showGuideModal}
        onClose={() => setShowGuideModal(false)}
        onNavigateTab={(t) => setActiveTab(t as any)}
        onOpenImport={() => setShowImportWizard(true)}
        onOpenNewTask={() => setShowNewTaskModal(true)}
      />

      {showImportWizard && (
        <RepoImportWizard
          onClose={() => setShowImportWizard(false)}
          onRefresh={onRefresh}
        />
      )}

      {showNewTaskModal && activeProject && (
        <NewTaskModal
          projectId={activeProject.id}
          onClose={() => setShowNewTaskModal(false)}
          onRefresh={onRefresh}
        />
      )}
    </div>
  );
};
