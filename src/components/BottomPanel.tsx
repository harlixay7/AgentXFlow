import React, { useState } from 'react';
import { Terminal, AlertCircle, CheckCircle, GitBranch, ChevronDown, ChevronUp } from 'lucide-react';
import { EventItem } from '../types';

interface BottomPanelProps {
  events: EventItem[];
}

export const BottomPanel: React.FC<BottomPanelProps> = ({ events }) => {
  const [activeTab, setActiveTab] = useState<'output' | 'problems' | 'verification' | 'git'>('output');
  const [isCollapsed, setIsCollapsed] = useState(false);

  return (
    <div className="bottom-debugger-panel" style={{ height: isCollapsed ? 32 : 180, transition: 'height 0.15s ease' }}>
      {/* Header */}
      <div className="bottom-panel-header">
        <div className="bottom-panel-tabs">
          <div
            className={`bottom-panel-tab ${activeTab === 'output' ? 'active' : ''}`}
            onClick={() => { setActiveTab('output'); setIsCollapsed(false); }}
          >
            <Terminal size={12} /> Event Stream ({events.length})
          </div>
          <div
            className={`bottom-panel-tab ${activeTab === 'problems' ? 'active' : ''}`}
            onClick={() => { setActiveTab('problems'); setIsCollapsed(false); }}
          >
            <AlertCircle size={12} /> Problems (0)
          </div>
          <div
            className={`bottom-panel-tab ${activeTab === 'verification' ? 'active' : ''}`}
            onClick={() => { setActiveTab('verification'); setIsCollapsed(false); }}
          >
            <CheckCircle size={12} /> Verification Runs
          </div>
          <div
            className={`bottom-panel-tab ${activeTab === 'git' ? 'active' : ''}`}
            onClick={() => { setActiveTab('git'); setIsCollapsed(false); }}
          >
            <GitBranch size={12} /> Git Integration
          </div>
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          <div style={{ fontSize: 10, color: 'var(--text-muted)', fontFamily: 'var(--font-mono)' }}>
            Viducia • Developed by harlixay7
          </div>
          <button
            onClick={() => setIsCollapsed(!isCollapsed)}
            style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}
            title={isCollapsed ? 'Expand panel' : 'Collapse panel'}
          >
            {isCollapsed ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
          </button>
        </div>
      </div>

      {/* Content */}
      {!isCollapsed && (
        <div className="bottom-panel-content">
          {activeTab === 'output' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
              {events.slice(-50).map((ev) => (
                <div key={ev.sequence} style={{ display: 'flex', gap: 10 }}>
                  <span style={{ color: 'var(--text-muted)' }}>#{ev.sequence}</span>
                  <span style={{ color: 'var(--text-dim)' }}>[{ev.timestamp.split('T')[1]?.split('.')[0] || ''}]</span>
                  <span style={{ color: 'var(--accent-blue)', fontWeight: 600 }}>{ev.event_type}</span>
                  <span style={{ color: 'var(--text-secondary)' }}>{ev.payload_json}</span>
                </div>
              ))}
            </div>
          )}

          {activeTab === 'problems' && (
            <div style={{ color: 'var(--text-muted)' }}>No active scope violations or compiler errors detected.</div>
          )}

          {activeTab === 'verification' && (
            <div style={{ color: 'var(--text-secondary)' }}>
              Coordinator Verification Workers: IDLE | Ready to execute cargo test / npm test on task submission.
            </div>
          )}

          {activeTab === 'git' && (
            <div style={{ color: 'var(--text-secondary)' }}>
              Dedicated Integration Worktree: <span style={{ color: 'var(--accent-blue)' }}>.agentxflow/integration</span> | Zero merges in root checkout.
            </div>
          )}
        </div>
      )}
    </div>
  );
};
