import React, { useState } from 'react';
import { X, Folder, CheckCircle, AlertCircle } from 'lucide-react';
import { coordinatorApi } from '../api/coordinator';
import { RepoInspectionResult } from '../types';

interface RepoImportWizardProps {
  onClose: () => void;
  onRefresh: () => void;
}

export const RepoImportWizard: React.FC<RepoImportWizardProps> = ({ onClose, onRefresh }) => {
  const [name, setName] = useState('');
  const [path, setPath] = useState('b:/AgentXFlow');
  const [masterSpec, setMasterSpec] = useState('Cross-Agent Engineering Coordinator');
  const [targetBranch, setTargetBranch] = useState('main');
  const [inspection, setInspection] = useState<RepoInspectionResult | null>(null);
  const [loading, setLoading] = useState(false);

  const handleBrowse = async () => {
    try {
      const selected = await coordinatorApi.pickFolder();
      if (selected) {
        setPath(selected);
        const basename = selected.replace(/\\/g, '/').split('/').filter(Boolean).pop() || '';
        if (basename && !name) {
          setName(basename.charAt(0).toUpperCase() + basename.slice(1));
        }

        // Run repository auto-inspection
        const inspectRes = await coordinatorApi.inspectRepository(selected);
        setInspection(inspectRes);
        if (inspectRes.active_branch) {
          setTargetBranch(inspectRes.active_branch);
        }
      }
    } catch (e) {
      console.error('Failed to pick/inspect folder:', e);
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
          width: 520,
          backgroundColor: 'var(--bg-surface)',
          border: '1px solid var(--border-medium)',
          borderRadius: 'var(--radius-lg)',
          padding: 18,
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
          <h3 style={{ fontSize: 13, fontWeight: 600, fontFamily: 'var(--font-mono)' }}>Import & Inspect Repository</h3>
          <button onClick={onClose} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}>
            <X size={15} />
          </button>
        </div>

        <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div>
            <label className="section-label">Directory Path</label>
            <div style={{ display: 'flex', gap: 6 }}>
              <input
                className="input-field"
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="b:/AgentXFlow"
                required
              />
              <button type="button" className="btn btn-secondary" onClick={handleBrowse}>
                <Folder size={13} /> Browse...
              </button>
            </div>
          </div>

          {/* Inspection Review Box */}
          {inspection && (
            <div style={{ padding: 10, backgroundColor: 'var(--bg-input)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-sm)', fontSize: 11 }}>
              <div style={{ fontWeight: 600, marginBottom: 4, color: inspection.is_git_repo ? 'var(--accent-green)' : 'var(--accent-yellow)', display: 'flex', alignItems: 'center', gap: 6 }}>
                {inspection.is_git_repo ? <CheckCircle size={13} /> : <AlertCircle size={13} />}
                {inspection.is_git_repo ? 'Git Repository Verified' : 'Uninitialized Directory (AgentXFlow will initialize Git)'}
              </div>
              <div style={{ color: 'var(--text-secondary)' }}>
                Languages: {inspection.languages.join(', ') || 'General'}<br />
                Package Managers: {inspection.package_managers.join(', ') || 'None'}<br />
                Test Commands: {inspection.test_scripts.join(', ') || 'Manual'}
              </div>
            </div>
          )}

          <div>
            <label className="section-label">Project Name</label>
            <input
              className="input-field"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. AgentXFlow Engine"
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
            <label className="section-label">Master Specification & Rules</label>
            <textarea
              className="input-field"
              rows={3}
              value={masterSpec}
              onChange={(e) => setMasterSpec(e.target.value)}
              placeholder="Core architectural constraints and engineering standards..."
            />
          </div>

          <div style={{ marginTop: 6, display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
            <button type="button" className="btn btn-secondary" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className="btn btn-primary" disabled={loading}>
              {loading ? 'Importing...' : 'Connect & Import'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};
