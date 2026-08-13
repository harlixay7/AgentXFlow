import React from 'react';
import { EventItem } from '../types';
import { Activity, Clock } from 'lucide-react';

interface ActivityViewProps {
  logs: EventItem[];
}

export const ActivityView: React.FC<ActivityViewProps> = ({ logs }) => {
  return (
    <div style={{ flex: 1, padding: 24, overflowY: 'auto' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 16 }}>
        <Activity size={18} style={{ color: 'var(--accent-blue)' }} />
        <h2 style={{ fontSize: 16, fontWeight: 700 }}>Audit Event Log & Pipeline Provenance</h2>
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {logs.map((log) => (
          <div
            key={log.event_id}
            style={{
              padding: '12px 16px',
              backgroundColor: 'var(--bg-surface)',
              border: '1px solid var(--border-subtle)',
              borderRadius: 'var(--radius-md)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
            }}
          >
            <div>
              <span
                style={{
                  fontWeight: 600,
                  color: 'var(--accent-blue)',
                  marginRight: 10,
                }}
              >
                [{log.event_type}]
              </span>
              <span>{log.payload_json}</span>
            </div>

            <div style={{ color: 'var(--text-muted)', fontSize: 11, display: 'flex', alignItems: 'center', gap: 4 }}>
              <Clock size={12} />
              {new Date(log.timestamp).toLocaleTimeString()}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};
