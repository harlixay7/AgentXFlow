import React, { useState } from 'react';
import { Task } from '../types';
import { GitMerge, ShieldCheck, CheckCircle2, AlertCircle, Info } from 'lucide-react';
import { coordinatorApi } from '../api/coordinator';

interface ReviewCenterProps {
  tasks: Task[];
  onRefresh: () => void;
}

export const ReviewCenter: React.FC<ReviewCenterProps> = ({ tasks, onRefresh }) => {
  const [feedback, setFeedback] = useState<{ message: string; type: 'success' | 'error' } | null>(null);
  const [queuedTaskIds, setQueuedTaskIds] = useState<Set<string>>(new Set());
  const reviewTasks = tasks.filter((t) => t.state === 'REVIEW' || t.state === 'MERGE_READY');

  React.useEffect(() => {
    let isMounted = true;
    const fetchQueue = async () => {
      if (reviewTasks.length === 0) return;
      const projectIds = Array.from(new Set(reviewTasks.map((t) => t.project_id)));
      const ids = new Set<string>();
      for (const pid of projectIds) {
        try {
          const items = await coordinatorApi.listMergeQueue(pid);
          items.filter((item) => !item.processed_at).forEach((item) => ids.add(item.task_id));
        } catch {
          // ignore
        }
      }
      if (isMounted) {
        setQueuedTaskIds(ids);
      }
    };
    fetchQueue();
    const interval = setInterval(fetchQueue, 3000);
    return () => {
      isMounted = false;
      clearInterval(interval);
    };
  }, [tasks]);

  const handleEnqueue = async (t: Task) => {
    setFeedback(null);
    try {
      await coordinatorApi.enqueueTaskById(t.project_id, t.id);
      setFeedback({
        message: `Task "${t.id}" has been authoritatively added to the Serialized Merge Queue!`,
        type: 'success',
      });
      onRefresh();
    } catch (e: any) {
      setFeedback({
        message: `Failed to enqueue task: ${e.toString()}`,
        type: 'error',
      });
    }
  };

  return (
    <div style={{ flex: 1, padding: 20, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 16 }}>
      <div style={{ borderBottom: '1px solid var(--border-medium)', paddingBottom: 14 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <ShieldCheck size={18} style={{ color: 'var(--accent-purple)' }} />
          <h2 style={{ fontSize: 15, fontWeight: 700, fontFamily: 'var(--font-mono)' }}>Review Center & Proof Bundles</h2>
        </div>
        <p style={{ color: 'var(--text-secondary)', fontSize: 11, marginTop: 4 }}>
          Tasks listed here have passed all mandatory steps and automated verification checks.
        </p>
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
          <span>{feedback.message}</span>
        </div>
      )}

      {reviewTasks.length === 0 ? (
        <div style={{ padding: 40, textAlign: 'center', backgroundColor: 'var(--bg-surface)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-md)', color: 'var(--text-muted)', fontSize: 12 }}>
          <CheckCircle2 size={24} style={{ color: 'var(--accent-green)', margin: '0 auto 8px auto' }} />
          <div style={{ fontWeight: 600, color: 'var(--text-primary)' }}>No tasks currently awaiting review</div>
          <p style={{ fontSize: 11, marginTop: 4 }}>When an agent submits code and passes automated verification tests, it will appear here for your sign-off.</p>
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          {reviewTasks.map((t) => {
            const isEnqueued = queuedTaskIds.has(t.id);
            return (
              <div
                key={t.id}
                style={{
                  backgroundColor: 'var(--bg-surface)',
                  border: '1px solid var(--border-medium)',
                  borderRadius: 'var(--radius-md)',
                  padding: 16,
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
                  <div>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                      <span className={`badge badge-${t.state}`} style={{ marginRight: 4 }}>{t.state}</span>
                      <span style={{ fontWeight: 700, fontSize: 13, fontFamily: 'var(--font-mono)', color: 'var(--accent-blue)', userSelect: 'text' }}>{t.id}: </span>
                      <span style={{ fontWeight: 600, fontSize: 13, userSelect: 'text' }}>{t.title}</span>
                    </div>
                    <div style={{ fontSize: 11, color: 'var(--text-secondary)', userSelect: 'text' }}>{t.description}</div>
                  </div>
                  {isEnqueued ? (
                    <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12, color: 'var(--accent-blue)', fontWeight: 600 }}>
                      <Info size={14} /> Enqueued in Merge Queue
                    </div>
                  ) : (
                    <button
                      className="btn btn-primary"
                      onClick={() => handleEnqueue(t)}
                      title="Move this verified task into the background merge queue"
                    >
                      <GitMerge size={13} /> Enqueue for 3-Way Merge
                    </button>
                  )}
                </div>

              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))', gap: 10, backgroundColor: 'var(--bg-card)', padding: 12, borderRadius: 'var(--radius-sm)', border: '1px solid var(--border-subtle)', fontSize: 11 }}>
                <div style={{ color: 'var(--accent-green)', display: 'flex', alignItems: 'center', gap: 6, fontWeight: 600 }} title="Coordinator verified test output against submitted commit HEAD">
                  <ShieldCheck size={14} /> Verification Passed
                </div>
                <div title="Target base commit SHA before task branch was created" style={{ userSelect: 'text' }}>
                  Base Commit: <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--accent-blue)', userSelect: 'text' }}>{t.base_sha ? t.base_sha.substring(0, 8) : 'RECORDED_AT_CLAIM'}</code>
                </div>
                <div title="Head commit SHA of the task branch" style={{ userSelect: 'text' }}>
                  Head Commit: <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--accent-purple)', userSelect: 'text' }}>{t.head_sha ? t.head_sha.substring(0, 8) : 'AWAITING_COMMIT'}</code>
                </div>
                <div title="Assigned agent responsible for this task" style={{ userSelect: 'text' }}>
                  Agent: <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-primary)', userSelect: 'text' }}>{t.assigned_agent_id || 'Coordinator'}</span>
                </div>
              </div>
            </div>
          );
        })}
        </div>
      )}
    </div>
  );
};
