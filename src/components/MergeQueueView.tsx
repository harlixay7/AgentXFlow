import React, { useState } from 'react';
import { MergeQueueItem } from '../types';
import { Play, RefreshCw, GitMerge, CheckCircle2, AlertCircle } from 'lucide-react';
import { coordinatorApi } from '../api/coordinator';

interface MergeQueueViewProps {
  queue: MergeQueueItem[];
  projectId: string;
  onRefresh: () => void;
}

export const MergeQueueView: React.FC<MergeQueueViewProps> = ({ queue, projectId, onRefresh }) => {
  const [feedback, setFeedback] = useState<{ message: string; type: 'success' | 'error' } | null>(null);

  const handleProcess = async (item: MergeQueueItem) => {
    setFeedback(null);
    try {
      const attempt = await coordinatorApi.processMergeById(projectId, item.id);
      if (attempt.simulation_passed) {
        setFeedback({
          message: `Task "${item.task_id}" successfully merged in disposable integration worktree and committed to ${item.target_branch}!`,
          type: 'success',
        });
      } else {
        setFeedback({
          message: `Merge conflict or verification failure for task "${item.task_id}": ${attempt.conflicts_json || 'Post-merge checks failed'}`,
          type: 'error',
        });
      }
      onRefresh();
    } catch (e: any) {
      setFeedback({
        message: `Integration error: ${e.toString()}`,
        type: 'error',
      });
    }
  };

  return (
    <div style={{ flex: 1, padding: 20, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 16 }}>
      {/* Header with Plain English Explanation */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', borderBottom: '1px solid var(--border-medium)', paddingBottom: 14 }}>
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <GitMerge size={18} style={{ color: 'var(--accent-blue)' }} />
            <h2 style={{ fontSize: 15, fontWeight: 700, fontFamily: 'var(--font-mono)' }}>Serialized Merge Queue</h2>
          </div>
          <p style={{ color: 'var(--text-secondary)', fontSize: 11, marginTop: 4, maxWidth: 700, lineHeight: 1.5 }}>
            <strong>How it works:</strong> Merge operations are simulated one-by-one inside an isolated app-data worktree. Merges <strong>never touch or dirty your local repository workspace</strong>. If tests pass and there are no conflicts, the target branch ref is advanced using atomic Compare-and-Swap (CAS).
          </p>
        </div>
        <button
          className="btn btn-secondary"
          onClick={onRefresh}
          title="Refresh current merge queue order and candidate statuses"
        >
          <RefreshCw size={12} /> Refresh Queue
        </button>
      </div>

      {feedback && (
        <div
          style={{
            padding: '10px 14px',
            borderRadius: 'var(--radius-sm)',
            fontSize: 12,
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            backgroundColor: feedback.type === 'success' ? 'rgba(34, 197, 94, 0.1)' : 'rgba(239, 68, 68, 0.1)',
            border: `1px solid ${feedback.type === 'success' ? 'var(--accent-green)' : 'var(--accent-red)'}`,
            color: feedback.type === 'success' ? 'var(--accent-green)' : 'var(--accent-red)',
          }}
        >
          {feedback.type === 'success' ? <CheckCircle2 size={14} /> : <AlertCircle size={14} />}
          <span style={{ userSelect: 'text' }}>{feedback.message}</span>
        </div>
      )}

      {queue.length === 0 ? (
        <div style={{ padding: 40, textAlign: 'center', backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-md)', color: 'var(--text-muted)', fontSize: 12 }}>
          <CheckCircle2 size={24} style={{ color: 'var(--accent-green)', margin: '0 auto 8px auto' }} />
          <div style={{ fontWeight: 600, color: 'var(--text-primary)' }}>Merge queue is clear</div>
          <p style={{ fontSize: 11, marginTop: 4 }}>All verified task branches have been merged into the main branch.</p>
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          {queue.map((item) => (
            <div
              key={item.id}
              style={{
                backgroundColor: 'var(--bg-surface)',
                border: '1px solid var(--border-medium)',
                borderRadius: 'var(--radius-md)',
                padding: 14,
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
              }}
            >
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                  <span style={{ fontWeight: 700, fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-blue)', userSelect: 'text' }}>#{item.position}</span>
                  <span
                    className={`badge ${item.status === 'READY' ? 'badge-READY' : item.status === 'MERGED' ? 'badge-DONE' : 'badge-BLOCKED'}`}
                    title={`Candidate merge status: ${item.status}`}
                  >
                    {item.status}
                  </span>
                  <span style={{ fontWeight: 600, fontFamily: 'var(--font-mono)', userSelect: 'text' }}>Task ID: {item.task_id}</span>
                </div>
                <div style={{ fontSize: 11, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', userSelect: 'text' }}>
                  Branch: <span style={{ color: 'var(--accent-blue)' }}>{item.branch_name}</span> → Target: <span style={{ color: 'var(--accent-green)' }}>{item.target_branch}</span>
                  <span style={{ marginLeft: 12, color: 'var(--text-muted)' }}>HEAD: {item.head_sha ? item.head_sha.substring(0, 8) : 'N/A'}</span>
                </div>
              </div>

              {item.status === 'READY' && (
                <button
                  className="btn btn-primary"
                  onClick={() => handleProcess(item)}
                  title="Execute background test-merge and CAS commit to target branch"
                >
                  <Play size={12} /> Process Integration
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
};
