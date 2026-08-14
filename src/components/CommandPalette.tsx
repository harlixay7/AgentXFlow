import React, { useState, useEffect } from 'react';
import { Search, Plus, LayoutGrid, GitMerge, Cpu, Activity, Plug, FolderPlus, CheckCircle } from 'lucide-react';

interface CommandPaletteProps {
  isOpen: boolean;
  onClose: () => void;
  onNavigateTab: (tab: string) => void;
  onOpenNewTaskModal: () => void;
  onOpenImportModal: () => void;
}

export const CommandPalette: React.FC<CommandPaletteProps> = ({
  isOpen,
  onClose,
  onNavigateTab,
  onOpenNewTaskModal,
  onOpenImportModal,
}) => {
  const [query, setQuery] = useState('');

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        if (isOpen) onClose();
      }
      if (e.key === 'Escape' && isOpen) {
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const commands = [
    { label: 'Create New Engineering Task', icon: Plus, action: () => { onClose(); onOpenNewTaskModal(); } },
    { label: 'Import / Connect Git Repository', icon: FolderPlus, action: () => { onClose(); onOpenImportModal(); } },
    { label: 'Go to Overview (Mission Control)', icon: Activity, action: () => { onClose(); onNavigateTab('overview'); } },
    { label: 'Go to Work (List / Board / DAG)', icon: LayoutGrid, action: () => { onClose(); onNavigateTab('work'); } },
    { label: 'Go to Registered Agents & ACP', icon: Cpu, action: () => { onClose(); onNavigateTab('agents'); } },
    { label: 'Go to Review Center & Proof Bundles', icon: CheckCircle, action: () => { onClose(); onNavigateTab('review'); } },
    { label: 'Go to Serialized Merge Queue', icon: GitMerge, action: () => { onClose(); onNavigateTab('merge_queue'); } },
    { label: 'Go to Authoritative MCP Gateway', icon: Plug, action: () => { onClose(); onNavigateTab('integrations'); } },
  ];

  const filtered = commands.filter((c) => c.label.toLowerCase().includes(query.toLowerCase()));

  return (
    <div className="command-palette-overlay" onClick={onClose}>
      <div className="command-palette-modal" onClick={(e) => e.stopPropagation()}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '10px 14px', borderBottom: '1px solid var(--border-medium)', backgroundColor: 'var(--bg-input)' }}>
          <Search size={14} style={{ color: 'var(--text-muted)' }} />
          <input
            autoFocus
            className="input-field"
            style={{ border: 'none', background: 'transparent', padding: 0, fontSize: 13, height: 'auto' }}
            placeholder="Type a command or search actions..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <span className="kbd-shortcut">ESC</span>
        </div>

        <div style={{ maxHeight: 280, overflowY: 'auto', padding: 6, display: 'flex', flexDirection: 'column', gap: 2 }}>
          {filtered.map((c, i) => (
            <div
              key={i}
              className="nav-item"
              style={{ padding: '8px 10px', fontSize: 12, borderRadius: 'var(--radius-sm)' }}
              onClick={c.action}
            >
              <c.icon size={13} style={{ color: 'var(--accent-blue)' }} />
              <span>{c.label}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
