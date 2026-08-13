use std::collections::{HashMap, HashSet, VecDeque};
use chrono::Utc;
use uuid::Uuid;
use crate::db::DbPool;
use crate::models::TaskDependency;

#[derive(Debug, Clone)]
pub struct DagEngine {
    db: DbPool,
}

impl DagEngine {
    pub fn new(db: DbPool) -> Self {
        Self { db }
    }

    pub fn add_dependency(
        &self,
        task_id: &str,
        depends_on_task_id: &str,
        dependency_type: &str,
    ) -> Result<TaskDependency, String> {
        if task_id == depends_on_task_id {
            return Err("A task cannot depend on itself".to_string());
        }

        // Check for cycle before adding
        if self.would_create_cycle(task_id, depends_on_task_id)? {
            return Err(format!("Adding dependency from '{}' to '{}' would create a circular dependency cycle", task_id, depends_on_task_id));
        }

        let conn = self.db.lock();
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO task_dependencies (id, task_id, depends_on_task_id, dependency_type, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, task_id, depends_on_task_id, dependency_type, now],
        ).map_err(|e| e.to_string())?;

        Ok(TaskDependency {
            id,
            task_id: task_id.to_string(),
            depends_on_task_id: depends_on_task_id.to_string(),
            dependency_type: dependency_type.to_string(),
            created_at: now,
        })
    }

    pub fn remove_dependency(&self, dependency_id: &str) -> Result<(), String> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM task_dependencies WHERE id = ?1", [dependency_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_dependencies_for_task(&self, task_id: &str) -> Result<Vec<TaskDependency>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT id, task_id, depends_on_task_id, dependency_type, created_at FROM task_dependencies WHERE task_id = ?1")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([task_id], |row| {
                Ok(TaskDependency {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    depends_on_task_id: row.get(2)?,
                    dependency_type: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut res = Vec::new();
        for r in rows {
            if let Ok(dep) = r {
                res.push(dep);
            }
        }
        Ok(res)
    }

    /// Returns true if all dependencies for the task have state == 'DONE'
    pub fn are_dependencies_satisfied(&self, task_id: &str) -> Result<bool, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT t.state FROM task_dependencies d JOIN tasks t ON d.depends_on_task_id = t.id WHERE d.task_id = ?1")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([task_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;

        for r in rows {
            if let Ok(state_str) = r {
                if state_str != "DONE" {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    /// Cycle detection using BFS / DFS reachability
    fn would_create_cycle(&self, task_id: &str, depends_on_task_id: &str) -> Result<bool, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT task_id, depends_on_task_id FROM task_dependencies")
            .map_err(|e| e.to_string())?;

        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;

        for r in rows {
            if let Ok((u, v)) = r {
                adj.entry(u).or_default().push(v);
            }
        }

        // Add hypothetical edge: task_id -> depends_on_task_id
        adj.entry(task_id.to_string()).or_default().push(depends_on_task_id.to_string());

        // Check if depends_on_task_id can reach task_id
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(depends_on_task_id.to_string());

        while let Some(curr) = queue.pop_front() {
            if curr == task_id {
                return Ok(true); // Cycle found!
            }
            if visited.insert(curr.clone()) {
                if let Some(neighbors) = adj.get(&curr) {
                    for n in neighbors {
                        queue.push_back(n.clone());
                    }
                }
            }
        }

        Ok(false)
    }
}
