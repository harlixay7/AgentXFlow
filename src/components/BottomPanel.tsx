import React, { useState } from 'react';
import { Terminal, AlertCircle, CheckCircle, GitBranch, ChevronDown, ChevronUp, ShieldCheck } from 'lucide-react';
import { EventItem } from '../types';

interface BottomPanelProps {
  events: EventItem[];
  syncError: string | null;
}

export const BottomPanel: React.FC<BottomPanelProps> = ({ events, syncError }) => {
  const [activeTab, setActiveTab] = useState<'output' | 'problems' | 'verification' | 'git'>('output');
  const [isCollapsed, setIsCollapsed] = useState(false);

  const getLogBadge = (eventType: string) => {
    const upper = eventType.toUpperCase();
    if (upper.includes('ERROR') || upper.includes('FAIL') || upper.includes('CONFLICT') || upper.includes('PANIC')) {
      return {
        label: '[HALT]',
        color: 'var(--accent-red)',
        bg: 'rgba(239, 68, 68, 0.12)',
        border: 'rgba(239, 68, 68, 0.3)',
      };
    }
    if (upper.includes('WARN') || upper.includes('REVERT') || upper.includes('TIMEOUT')) {
      return {
        label: '[WARN]',
        color: 'var(--accent-amber)',
        bg: 'rgba(245, 166, 35, 0.12)',
        border: 'rgba(245, 166, 35, 0.3)',
      };
    }
    if (upper.includes('SUCCESS') || upper.includes('MERGED') || upper.includes('DONE') || upper.includes('VERIFIED')) {
      return {
        label: '[DONE]',
        color: 'var(--accent-mint)',
        bg: 'rgba(46, 204, 113, 0.12)',
        border: 'rgba(46, 204, 113, 0.3)',
      };
    }
    return {
      label: '[INFO]',
      color: 'var(--accent-primary)',
      bg: 'rgba(0, 216, 255, 0.10)',
      border: 'rgba(0, 216, 255, 0.25)',
    };
  };

  return (
    <div
      className="bottom-debugger-panel"
      style={{
        height: isCollapsed ? 32 : 180,
        transition: 'height var(--duration-tactical) var(--ease-tactical)',
      }}
    >
      {/* Header */}
      <div className="bottom-panel-header">
        <div className="bottom-panel-tabs">
          <div
            className={`bottom-panel-tab ${activeTab === 'output' ? 'active' : ''}`}
            onClick={() => {
              setActiveTab('output');
              setIsCollapsed(false);
            }}
          >
            <Terminal size={12} />
            <span>Event Stream</span>
            <span
              style={{
                fontFamily: 'var(--font-mono)',
                fontSize: 10,
                color: 'var(--text-muted)',
                fontVariantNumeric: 'tabular-nums',
              }}
            >
              ({events.length})
            </span>
          </div>
          <div
            className={`bottom-panel-tab ${activeTab === 'problems' ? 'active' : ''}`}
            onClick={() => {
              setActiveTab('problems');
              setIsCollapsed(false);
            }}
          >
            <AlertCircle size={12} style={{ color: syncError ? 'var(--accent-red)' : undefined }} />
            <span>Problems</span>
            <span
              style={{
                fontFamily: 'var(--font-mono)',
                fontSize: 10,
                color: syncError ? 'var(--accent-red)' : 'var(--text-muted)',
                fontVariantNumeric: 'tabular-nums',
              }}
            >
              ({syncError ? 1 : 0})
            </span>
          </div>
          <div
            className={`bottom-panel-tab ${activeTab === 'verification' ? 'active' : ''}`}
            onClick={() => {
              setActiveTab('verification');
              setIsCollapsed(false);
            }}
          >
            <CheckCircle size={12} />
            <span>Verification Runs</span>
          </div>
          <div
            className={`bottom-panel-tab ${activeTab === 'git' ? 'active' : ''}`}
            onClick={() => {
              setActiveTab('git');
              setIsCollapsed(false);
            }}
          >
            <GitBranch size={12} />
            <span>Git Integration</span>
          </div>
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          <div style={{ fontSize: 10, color: 'var(--text-muted)', fontFamily: 'var(--font-mono)', fontVariantNumeric: 'tabular-nums' }}>
            DAEMON 127.0.0.1:7890 • ARCH: CAS-ISOLATED
          </div>
          <button
            onClick={() => setIsCollapsed(!isCollapsed)}
            style={{
              background: 'none',
              border: 'none',
              color: 'var(--text-secondary)',
              cursor: 'pointer',
              display: 'flex',
              alignItems: 'center',
              padding: 2,
            }}
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
            <div style={{ display: 'flex', flexDirection: 'column', gap: 2, fontVariantNumeric: 'tabular-nums' }}>
              {events.length === 0 ? (
                <div style={{ color: 'var(--text-muted)', fontStyle: 'italic', padding: '6px 0' }}>
                  No event records captured yet. Coordinator event bus listening on 127.0.0.1:7890...
                </div>
              ) : (
                events.slice(-60).map((ev) => {
                  const badge = getLogBadge(ev.event_type);
                  return (
                    <div
                      key={ev.sequence}
                      style={{
                        display: 'flex',
                        alignItems: 'baseline',
                        gap: 8,
                        padding: '2px 4px',
                        borderRadius: 'var(--radius-xs)',
                        lineHeight: 1.4,
                      }}
                      onMouseEnter={(e) => {
                        e.currentTarget.style.backgroundColor = 'rgba(255, 255, 255, 0.03)';
                      }}
                      onMouseLeave={(e) => {
                        e.currentTarget.style.backgroundColor = 'transparent';
                      }}
                    >
                      <span style={{ color: 'var(--text-muted)', width: 44, flexShrink: 0, userSelect: 'none' }}>
                        #{String(ev.sequence).padStart(4, '0')}
                      </span>
                      <span style={{ color: 'var(--text-tertiary)', width: 68, flexShrink: 0, userSelect: 'none' }}>
                        [{ev.timestamp.split('T')[1]?.split('.')[0] || ''}]
                      </span>
                      <span
                        style={{
                          fontSize: 9,
                          fontFamily: 'var(--font-mono)',
                          fontWeight: 700,
                          color: badge.color,
                          backgroundColor: badge.bg,
                          border: `1px solid ${badge.border}`,
                          padding: '1px 5px',
                          borderRadius: 'var(--radius-xs)',
                          flexShrink: 0,
                          lineHeight: 1.2,
                        }}
                      >
                        {badge.label}
                      </span>
                      <span style={{ color: 'var(--accent-primary)', fontWeight: 600, flexShrink: 0 }}>
                        {ev.event_type}:
                      </span>
                      <span style={{ color: 'var(--text-secondary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {ev.payload_json}
                      </span>
                    </div>
                  );
                })
              )}
            </div>
          )}

          {activeTab === 'problems' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {syncError ? (
                <div
                  style={{
                    padding: '8px 12px',
                    backgroundColor: 'rgba(239, 68, 68, 0.1)',
                    border: '1px solid var(--accent-red)',
                    borderRadius: 'var(--radius-xs)',
                    color: 'var(--accent-red)',
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                  }}
                >
                  <AlertCircle size={14} />
                  <span>Sync Error: {syncError}</span>
                </div>
              ) : (
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--accent-mint)' }}>
                  <ShieldCheck size={14} />
                  <span>Zero compile errors, scope conflicts, or invariant violations detected. Systems nominal.</span>
                </div>
              )}
            </div>
          )}

          {activeTab === 'verification' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6, color: 'var(--text-secondary)' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--text-primary)', fontWeight: 600 }}>
                <span className="live-dot" />
                <span>Coordinator Verification Engine</span>
              </div>
              <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                Verification Workers: <span style={{ color: 'var(--accent-mint)', fontWeight: 600 }}>READY</span> • Standard test runner configured (cargo test / npm test).
              </div>
              <div style={{ fontSize: 10, color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>
                Evidence generation: Automated SHA-256 commit hashing, isolated cargo test execution, diff bundle verification.
              </div>
            </div>
          )}

          {activeTab === 'git' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6, color: 'var(--text-secondary)' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--text-primary)', fontWeight: 600 }}>
                <GitBranch size={13} style={{ color: 'var(--accent-primary)' }} />
                <span>Git Multi-Worktree Isolation Fabric</span>
              </div>
              <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                Dedicated Integration Worktree: <code style={{ color: 'var(--accent-primary)', fontFamily: 'var(--font-mono)' }}>.agentxflow/integration</code>
              </div>
              <div style={{ fontSize: 10, color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>
                Contract Guarantee: Zero merges performed in the root working directory. Every task branch is verified in a clean disposable worktree prior to serialized CAS ref advancement.
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
};

