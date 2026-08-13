import React, { useState } from 'react';
import { X, Folder, Plus } from 'lucide-react';
import { coordinatorApi } from '../api/coordinator';

interface NewProjectModalProps {
  onClose: () => void;
  onRefresh: () => void;
}

export const NewProjectModal: React.FC<NewProjectModalProps> = ({ onClose, onRefresh }) => {
  const [name, setName] = useState('');
  const [path, setPath] = useState('b:/AgentXFlow');
  const [masterSpec, setMasterSpec] = useState('Cross-Agent Engineering Coordinator');
  const [targetBranch, setTargetBranch] = useState('main');
  const [loading, setLoading] = useState(false);

  const handleBrowseDisk = async () => {
    try {
      const selected = await coordinatorApi.pickFolder();
      if (selected) {
        setPath(selected);
        const basename = selected.replace(/\\/g, '/').split('/').filter(Boolean).pop() || '';
        if (basename && !name) {
          setName(basename.charAt(0).toUpperCase() + basename.slice(1));
        }
      }
    } catch (e) {
      console.error('Failed to pick folder:', e);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !path.trim()) return;

    setLoading(true);
    try {
      await coordinatorApi.createProject(name, path, masterSpec, targetBranch);
      onRefresh();
      onClose();
    } catch (err: any) {
      alert(err.toString());
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      style={{
        position: 'fixed',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        backgroundColor: 'rgba(1, 4, 9, 0.8)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
      }}
    >
      <div
        style={{
          width: 500,
          backgroundColor: 'var(--bg-surface)',
          border: '1px solid var(--border-medium)',
          borderRadius: 'var(--radius-lg)',
          padding: 20,
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <h3 style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-primary)', fontFamily: 'var(--font-mono)' }}>
            Import Repository
          </h3>
          <button onClick={onClose} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}>
            <X size={16} />
          </button>
        </div>

        <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div>
            <label className="section-label">Directory Path</label>
            <div style={{ display: 'flex', gap: 8, marginTop: 4 }}>
              <input
                className="input-field"
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="b:/AgentXFlow"
                required
              />
              <button
                type="button"
                className="btn btn-secondary"
                onClick={handleBrowseDisk}
                style={{ whiteSpace: 'nowrap' }}
              >
                <Folder size={13} />
                Browse...
              </button>
            </div>
          </div>

          <div>
            <label className="section-label">Project Name</label>
            <input
              className="input-field"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. AgentXFlow Core"
              required
            />
          </div>

          <div>
            <label className="section-label">Target Integration Branch</label>
            <input
              className="input-field"
              value={targetBranch}
              onChange={(e) => setTargetBranch(e.target.value)}
              placeholder="main"
            />
          </div>

          <div>
            <label className="section-label">Master Specification</label>
            <textarea
              className="input-field"
              rows={3}
              value={masterSpec}
              onChange={(e) => setMasterSpec(e.target.value)}
              placeholder="Engineering spec and rules..."
            />
          </div>

          <div style={{ marginTop: 8, display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
            <button type="button" className="btn btn-secondary" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className="btn btn-primary" disabled={loading}>
              <Plus size={13} />
              {loading ? 'Importing...' : 'Import Repository'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};
