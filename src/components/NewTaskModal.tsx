import React, { useState } from 'react';
import { X, Plus, Trash } from 'lucide-react';
import { coordinatorApi } from '../api/coordinator';

interface NewTaskModalProps {
  projectId: string;
  onClose: () => void;
  onRefresh: () => void;
}

export const NewTaskModal: React.FC<NewTaskModalProps> = ({ projectId, onClose, onRefresh }) => {
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [priority, setPriority] = useState('MEDIUM');
  const [steps, setSteps] = useState<Array<{ title: string; desc: string; isMandatory: boolean }>>([
    { title: 'Analyze existing codebase & declare write scope', desc: '', isMandatory: true },
    { title: 'Implement requested feature or fix in worktree', desc: '', isMandatory: true },
    { title: 'Run automated tests & capture evidence', desc: '', isMandatory: true },
  ]);
  const [criteria, setCriteria] = useState<string[]>([
    'All tests pass cleanly without errors',
    'No scope violations outside declared write locks',
  ]);


  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim()) return;

    const formattedSteps: Array<[string, string, boolean]> = steps.map((s) => [s.title, s.desc, s.isMandatory]);
    await coordinatorApi.createTask(projectId, title, description, priority, formattedSteps, criteria);
    onRefresh();
    onClose();
  };

  return (
    <div
      style={{
        position: 'fixed',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        backgroundColor: 'rgba(0, 0, 0, 0.7)',
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
          maxHeight: '90vh',
          overflowY: 'auto',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 16 }}>
          <h3 style={{ fontSize: 16, fontWeight: 700 }}>Create New Engineering Task</h3>
          <button onClick={onClose} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}>
            <X size={16} />
          </button>
        </div>

        <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div>
            <label className="section-label">Task Title</label>
            <input
              className="input-field"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="e.g. AUTH-04: Add Bearer Auth Validation"
              required
            />
          </div>

          <div>
            <label className="section-label">Description / Prompt</label>
            <textarea
              className="input-field"
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Detailed instructions for the agent..."
            />
          </div>

          <div>
            <label className="section-label">Priority</label>
            <select className="input-field" value={priority} onChange={(e) => setPriority(e.target.value)}>
              <option value="LOW">LOW</option>
              <option value="MEDIUM">MEDIUM</option>
              <option value="HIGH">HIGH</option>
              <option value="CRITICAL">CRITICAL</option>
            </select>
          </div>

          {/* Mandatory Steps Manager */}
          <div>
            <label className="section-label">Required Verification Steps</label>
            {steps.map((st, idx) => (
              <div key={idx} style={{ display: 'flex', gap: 6, marginBottom: 6 }}>
                <input
                  className="input-field"
                  value={st.title}
                  onChange={(e) => {
                    const next = [...steps];
                    next[idx].title = e.target.value;
                    setSteps(next);
                  }}
                />
                <button
                  type="button"
                  onClick={() => setSteps(steps.filter((_, i) => i !== idx))}
                  style={{ background: 'none', border: 'none', color: 'var(--accent-danger)', cursor: 'pointer' }}
                >
                  <Trash size={14} />
                </button>
              </div>
            ))}
            <button
              type="button"
              className="btn btn-secondary"
              style={{ fontSize: 11, padding: '4px 8px' }}
              onClick={() => setSteps([...steps, { title: 'New step', desc: '', isMandatory: true }])}
            >
              <Plus size={12} /> Add Step
            </button>
          </div>

          {/* Acceptance Criteria Manager */}
          <div>
            <label className="section-label">Acceptance Criteria</label>
            {criteria.map((crit, idx) => (
              <div key={idx} style={{ display: 'flex', gap: 6, marginBottom: 6 }}>
                <input
                  className="input-field"
                  value={crit}
                  onChange={(e) => {
                    const next = [...criteria];
                    next[idx] = e.target.value;
                    setCriteria(next);
                  }}
                />
                <button
                  type="button"
                  onClick={() => setCriteria(criteria.filter((_, i) => i !== idx))}
                  style={{ background: 'none', border: 'none', color: 'var(--accent-danger)', cursor: 'pointer' }}
                >
                  <Trash size={14} />
                </button>
              </div>
            ))}
            <button
              type="button"
              className="btn btn-secondary"
              style={{ fontSize: 11, padding: '4px 8px' }}
              onClick={() => setCriteria([...criteria, 'New acceptance criterion'])}
            >
              <Plus size={12} /> Add Criterion
            </button>
          </div>

          <div style={{ marginTop: 12, display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
            <button type="button" className="btn btn-secondary" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className="btn btn-primary">
              Create Task
            </button>
          </div>

        </form>
      </div>
    </div>
  );
};
