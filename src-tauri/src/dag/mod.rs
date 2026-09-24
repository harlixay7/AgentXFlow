use crate::db::DbPool;
use crate::models::TaskDependency;
use chrono::Utc;
use std::collections::{HashMap, HashSet, VecDeque};
use tracing::warn;
use uuid::Uuid;

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

        let mut conn = self.db.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        // Check for cycle within the active transaction
        if Self::would_create_cycle_on_conn(&tx, task_id, depends_on_task_id)? {
            return Err(format!(
                "Adding dependency from '{}' to '{}' would create a circular dependency cycle",
                task_id, depends_on_task_id
            ));
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        tx.execute(
            "INSERT INTO task_dependencies (id, task_id, depends_on_task_id, dependency_type, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, task_id, depends_on_task_id, dependency_type, now],
        ).map_err(|e| e.to_string())?;

        tx.commit()
            .map_err(|e| format!("Failed to commit dependency transaction: {}", e))?;
        drop(conn);

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
        conn.execute(
            "DELETE FROM task_dependencies WHERE id = ?1",
            [dependency_id],
        )
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
        for dep in rows {
            res.push(dep.map_err(|e| format!("Failed to read dependency row: {}", e))?);
        }
        Ok(res)
    }

    pub fn get_dependencies_for_project(&self, project_id: &str) -> Result<Vec<TaskDependency>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT d.id, d.task_id, d.depends_on_task_id, d.dependency_type, d.created_at
                      FROM task_dependencies d
                      JOIN tasks t ON d.task_id = t.id
                      WHERE t.project_id = ?1")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([project_id], |row| {
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
        for dep in rows {
            res.push(dep.map_err(|e| format!("Failed to read dependency row: {}", e))?);
        }
        Ok(res)
    }

    /// Returns true if all blocking dependencies for the task have state == 'DONE'.
    /// BLOCKS and PARENT_CHILD gate scheduling; RELATED_TO is informational and never
    /// blocks. Any other stored dependency_type is treated as blocking (conservative).
    pub fn are_dependencies_satisfied(&self, task_id: &str) -> Result<bool, String> {
        let conn = self.db.lock();
        Self::are_dependencies_satisfied_on_conn(&conn, task_id)
    }

    /// Connection-aware dependency satisfaction check for use within active transactions.
    pub fn are_dependencies_satisfied_on_conn(
        conn: &rusqlite::Connection,
        task_id: &str,
    ) -> Result<bool, String> {
        let unknown_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_dependencies WHERE task_id = ?1 AND dependency_type NOT IN ('BLOCKS', 'PARENT_CHILD', 'RELATED_TO')",
                [task_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if unknown_count > 0 {
            warn!(
                "task '{}' has {} dependencies with an unknown dependency_type; treating them as blocking",
                task_id, unknown_count
            );
        }

        let mut stmt = conn
            .prepare("SELECT t.state FROM task_dependencies d JOIN tasks t ON d.depends_on_task_id = t.id WHERE d.task_id = ?1 AND d.dependency_type != 'RELATED_TO'")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([task_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;

        for state_res in rows {
            let state_str =
                state_res.map_err(|e| format!("Failed to read dependency state: {}", e))?;
            if state_str != "DONE" {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Cycle detection using BFS / DFS reachability
    pub fn would_create_cycle(
        &self,
        task_id: &str,
        depends_on_task_id: &str,
    ) -> Result<bool, String> {
        let conn = self.db.lock();
        Self::would_create_cycle_on_conn(&conn, task_id, depends_on_task_id)
    }

    fn would_create_cycle_on_conn(
        conn: &rusqlite::Connection,
        task_id: &str,
        depends_on_task_id: &str,
    ) -> Result<bool, String> {
        let mut stmt = conn
            .prepare("SELECT task_id, depends_on_task_id FROM task_dependencies")
            .map_err(|e| e.to_string())?;

        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;

        for edge_res in rows {
            let (u, v) = edge_res.map_err(|e| format!("Failed to read graph edge: {}", e))?;
            adj.entry(u).or_default().push(v);
        }

        // Add hypothetical edge: task_id -> depends_on_task_id
        adj.entry(task_id.to_string())
            .or_default()
            .push(depends_on_task_id.to_string());

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

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_dag() -> DagEngine {
        let dir = std::env::temp_dir().join(format!("test_dag_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test_dag.db");
        let pool = DbPool::new(&db_path).unwrap();

        let conn = pool.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
            [&now],
        ).unwrap();
        for task_id in ["task-1", "task-A", "task-B", "task-C"] {
            conn.execute(
                "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES (?1, 'p1', 'T', 'Desc', 'BACKLOG', 'NONE', 'MEDIUM', 0, ?2, ?2)",
                rusqlite::params![task_id, now],
            ).unwrap();
        }
        drop(conn);

        DagEngine::new(pool)
    }

    #[test]
    fn test_dag_self_dependency_rejected() {
        let dag = setup_test_dag();
        let res = dag.add_dependency("task-1", "task-1", "BLOCKS");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("cannot depend on itself"));
    }

    #[test]
    fn test_dag_cycle_detection_direct_and_transitive() {
        let dag = setup_test_dag();
        // A depends on B
        assert!(dag.add_dependency("task-A", "task-B", "BLOCKS").is_ok());
        // B depends on C
        assert!(dag.add_dependency("task-B", "task-C", "BLOCKS").is_ok());

        // C depends on A -> would create cycle A -> B -> C -> A
        let res = dag.add_dependency("task-C", "task-A", "BLOCKS");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("circular dependency cycle"));

        // Direct cycle B depends on A
        let direct_res = dag.add_dependency("task-B", "task-A", "BLOCKS");
        assert!(direct_res.is_err());
        assert!(direct_res
            .unwrap_err()
            .contains("circular dependency cycle"));
    }
}
