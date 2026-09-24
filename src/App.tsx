import { useState, useEffect, useRef } from 'react';
import { Project, Task, Agent, MergeQueueItem, EventItem, TaskDependency } from './types';
import { coordinatorApi } from './api/coordinator';
import { WorkbenchShell } from './components/WorkbenchShell';

export function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [activeProject, setActiveProject] = useState<Project | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [mergeQueue, setMergeQueue] = useState<MergeQueueItem[]>([]);
  const [events, setEvents] = useState<EventItem[]>([]);
  const [dependencies] = useState<TaskDependency[]>([]);
  const [selectedTask, setSelectedTask] = useState<Task | null>(null);
  const [syncError, setSyncError] = useState<string | null>(null);

  const lastSeqRef = useRef<number>(0);
  const loadSeqRef = useRef<number>(0);

  // Load data with in-flight guard against stale state updates
  const loadData = async (projectId?: string) => {
    const seq = ++loadSeqRef.current;
    try {
      const projList = await coordinatorApi.listProjects();
      if (seq !== loadSeqRef.current) return;
      setProjects(projList);
      if (projList.length > 0 && !activeProject) {
        setActiveProject(projList[0]);
      }

      const activeProjId = projectId || activeProject?.id || (projList.length > 0 ? projList[0].id : '');
      if (activeProjId) {
        const [taskList, agentList, queueList] = await Promise.all([
          coordinatorApi.listTasks(activeProjId),
          coordinatorApi.listAgents(),
          coordinatorApi.listMergeQueue(activeProjId),
        ]);
        if (seq !== loadSeqRef.current) return;
        setTasks(taskList);
        setAgents(agentList);
        setMergeQueue(queueList);
      }
      if (seq === loadSeqRef.current) setSyncError(null);
    } catch (e) {
      if (seq !== loadSeqRef.current) return;
      setSyncError(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    loadData();
  }, [activeProject?.id]);

  // High-frequency lightweight sequence stream polling (replaces heavy 4s full-state poll)
  useEffect(() => {
    const streamInterval = setInterval(async () => {
      try {
        const newEvents = await coordinatorApi.getEventsAfter(lastSeqRef.current);
        if (newEvents && newEvents.length > 0) {
          setEvents((prev) => [...prev, ...newEvents].slice(-300));
          const maxSeq = Math.max(...newEvents.map((e) => e.sequence));
          lastSeqRef.current = maxSeq;
          // Trigger targeted data refresh when meaningful events occur
          // Event-gap (500 rows) triggers a full catch-up refetch
          loadData(activeProject?.id);
          setSyncError(null);
        }
      } catch (e) {
        setSyncError(e instanceof Error ? e.message : String(e));
      }
    }, 1000);

    return () => clearInterval(streamInterval);
  }, [activeProject?.id]);

  return (
    <WorkbenchShell
      projects={projects}
      activeProject={activeProject}
      tasks={tasks}
      agents={agents}
      mergeQueue={mergeQueue}
      events={events}
      dependencies={dependencies}
      selectedTask={selectedTask}
      syncError={syncError}
      onSelectProject={(p) => setActiveProject(p)}
      onSelectTask={(t) => setSelectedTask(t)}
      onRefresh={() => loadData()}
    />
  );
}

export default App;
