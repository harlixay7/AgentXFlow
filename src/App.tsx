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

  const lastSeqRef = useRef<number>(0);

  // Load initial data
  const loadData = async () => {
    try {
      const projList = await coordinatorApi.listProjects();
      setProjects(projList);
      if (projList.length > 0 && !activeProject) {
        setActiveProject(projList[0]);
      }

      const activeProjId = activeProject?.id || (projList.length > 0 ? projList[0].id : '');
      if (activeProjId) {
        const [taskList, agentList, queueList] = await Promise.all([
          coordinatorApi.listTasks(activeProjId),
          coordinatorApi.listAgents(),
          coordinatorApi.listMergeQueue(activeProjId),
        ]);
        setTasks(taskList);
        setAgents(agentList);
        setMergeQueue(queueList);
      }
    } catch (e) {
      console.error('Failed to load AgentXFlow data:', e);
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
          loadData();
        }
      } catch (e) {
        // silent
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
      onSelectProject={(p) => setActiveProject(p)}
      onSelectTask={(t) => setSelectedTask(t)}
      onRefresh={loadData}
    />
  );
}

export default App;
