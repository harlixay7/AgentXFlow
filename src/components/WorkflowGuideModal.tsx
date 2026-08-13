import React from 'react';
import { X, FolderGit2, Cpu, GitMerge, ShieldCheck, Terminal } from 'lucide-react';

interface WorkflowGuideModalProps {
  isOpen: boolean;
  onClose: () => void;
  onNavigateTab: (tab: string) => void;
  onOpenImport: () => void;
  onOpenNewTask: () => void;
}

export const WorkflowGuideModal: React.FC<WorkflowGuideModalProps> = ({
  isOpen,
  onClose,
  onNavigateTab,
  onOpenImport,
  onOpenNewTask,
}) => {
  if (!isOpen) return null;

  return (
    <div
      className="modal-backdrop"
      onClick={onClose}
      style={{
        position: 'fixed',
        inset: 0,
        backgroundColor: 'rgba(1, 4, 9, 0.75)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1100,
        padding: 20,
      }}
    >
      <div
        style={{
          width: '100%',
          maxWidth: 720,
          maxHeight: '90vh',
          backgroundColor: 'var(--bg-surface)',
          border: '1px solid var(--border-bright)',
          borderRadius: 'var(--radius-lg)',
          display: 'flex',
          flexDirection: 'column',
          boxShadow: '0 20px 40px rgba(0, 0, 0, 0.8)',
          overflow: 'hidden',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div
          style={{
            padding: '16px 20px',
            borderBottom: '1px solid var(--border-medium)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            backgroundColor: 'var(--bg-input)',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <div
              style={{
                width: 32,
                height: 32,
                borderRadius: 6,
                backgroundColor: 'rgba(88, 166, 255, 0.15)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: 'var(--accent-blue)',
              }}
            >
              <Terminal size={18} />
            </div>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <h3 style={{ margin: 0, fontSize: 15, fontWeight: 600, color: 'var(--text-primary)' }}>
                  AgentXFlow: Visual Multi-Agent Architecture
                </h3>
                <span
                  style={{
                    fontSize: 9,
                    fontFamily: 'var(--font-mono)',
                    backgroundColor: 'rgba(88, 166, 255, 0.15)',
                    color: 'var(--accent-blue)',
                    padding: '1px 6px',
                    borderRadius: 4,
                  }}
                >
                  by Viducia
                </span>
              </div>
              <p style={{ margin: 0, fontSize: 11, color: 'var(--text-muted)' }}>
                How AgentXFlow coordinates AI agents, isolated Git worktrees, write scopes, and merged builds.
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            style={{
              background: 'none',
              border: 'none',
              color: 'var(--text-muted)',
              cursor: 'pointer',
              padding: 4,
              borderRadius: 'var(--radius-sm)',
            }}
            title="Close guide (Esc)"
          >
            <X size={16} />
          </button>
        </div>

        {/* Content Body */}
        <div style={{ padding: 20, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 16, fontSize: 12 }}>
          {/* Step 1 */}
          <div className="guide-step-card">
            <div className="guide-step-num">1</div>
            <div style={{ flex: 1 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
                <h3 style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-primary)' }}>
                  Connect Your Git Repository
                </h3>
                <button
                  className="btn btn-secondary"
                  style={{ height: 24, fontSize: 11, padding: '2px 8px' }}
                  onClick={() => { onClose(); onOpenImport(); }}
                  title="Click to select a local folder from your disk"
                >
                  <FolderGit2 size={12} /> Open Import Wizard
                </button>
              </div>
              <p style={{ color: 'var(--text-secondary)', lineHeight: 1.5 }}>
                Click <strong>"Import..."</strong> in the top-left to select any folder on your computer. AgentXFlow inspects your language, test scripts, and build tools automatically. If Git isn't initialized yet, you can initialize it with one click.
              </p>
            </div>
          </div>

          {/* Step 2 */}
          <div className="guide-step-card">
            <div className="guide-step-num">2</div>
            <div style={{ flex: 1 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
                <h3 style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-primary)' }}>
                  Connect Your AI Agents (Codex, Claude, OpenCode, Antigravity)
                </h3>
                <button
                  className="btn btn-secondary"
                  style={{ height: 24, fontSize: 11, padding: '2px 8px' }}
                  onClick={() => { onClose(); onNavigateTab('integrations'); }}
                  title="View MCP server URL and ready-to-copy JSON configuration"
                >
                  <Cpu size={12} /> View MCP Settings
                </button>
              </div>
              <p style={{ color: 'var(--text-secondary)', lineHeight: 1.5 }}>
                AgentXFlow runs an internal MCP server on <code>127.0.0.1:7890</code>. Copy the 1-click config into your agent's config file (e.g. <code>.mcp.json</code> or Claude Code settings). Your agents can now see tasks, claim assignments, and submit code.
              </p>
            </div>
          </div>

          {/* Step 3 */}
          <div className="guide-step-card">
            <div className="guide-step-num">3</div>
            <div style={{ flex: 1 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
                <h3 style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-primary)' }}>
                  Create Tasks & Assign Exclusive File Locks
                </h3>
                <button
                  className="btn btn-primary"
                  style={{ height: 24, fontSize: 11, padding: '2px 8px' }}
                  onClick={() => { onClose(); onOpenNewTask(); }}
                  title="Open the task creation form"
                >
                  Create New Task
                </button>
              </div>
              <p style={{ color: 'var(--text-secondary)', lineHeight: 1.5 }}>
                Create engineering tasks with clear acceptance criteria. Each assigned agent works in an <strong>isolated Git worktree</strong> (inside <code>.agentxflow/worktrees/</code>) and acquires exclusive write scope locks so multiple agents never overwrite each other's work.
              </p>
            </div>
          </div>

          {/* Step 4 */}
          <div className="guide-step-card">
            <div className="guide-step-num">4</div>
            <div style={{ flex: 1 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
                <h3 style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-primary)' }}>
                  Authoritative Verification & Proof Bundles
                </h3>
                <button
                  className="btn btn-secondary"
                  style={{ height: 24, fontSize: 11, padding: '2px 8px' }}
                  onClick={() => { onClose(); onNavigateTab('review'); }}
                  title="View verified tasks awaiting review"
                >
                  <ShieldCheck size={12} /> Open Review Center
                </button>
              </div>
              <p style={{ color: 'var(--text-secondary)', lineHeight: 1.5 }}>
                Agent claims are never blindly trusted. AgentXFlow independently runs your test suite (<code>cargo test</code>, <code>npm test</code>, linters) in the background. When all tests pass, an immutable SHA-256 <strong>Proof Bundle</strong> is generated.
              </p>
            </div>
          </div>

          {/* Step 5 */}
          <div className="guide-step-card">
            <div className="guide-step-num">5</div>
            <div style={{ flex: 1 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
                <h3 style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-primary)' }}>
                  Serialized Merge Queue (Zero Broken Builds on Main)
                </h3>
                <button
                  className="btn btn-secondary"
                  style={{ height: 24, fontSize: 11, padding: '2px 8px' }}
                  onClick={() => { onClose(); onNavigateTab('merge_queue'); }}
                  title="View active merge queue"
                >
                  <GitMerge size={12} /> View Merge Queue
                </button>
              </div>
              <p style={{ color: 'var(--text-secondary)', lineHeight: 1.5 }}>
                Verified code moves into the Serialized Merge Queue. Merges are simulated in a dedicated hidden worktree (<code>.agentxflow/integration/</code>). If tests pass and there are no conflicts, changes are safely integrated into your <code>main</code> branch.
              </p>
            </div>
          </div>
        </div>

        {/* Footer */}
        <div
          style={{
            padding: '12px 20px',
            borderTop: '1px solid var(--border-medium)',
            backgroundColor: 'var(--bg-input)',
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
          }}
        >
          <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>
            Tip: Press <kbd className="kbd-shortcut">Ctrl+K</kbd> anywhere to access any action instantly.
          </span>
          <button className="btn btn-primary" onClick={onClose}>
            Got It, Let's Build!
          </button>
        </div>
      </div>
    </div>
  );
};
