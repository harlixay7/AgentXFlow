use crate::db::DbPool;

#[test]
fn test_second_coordinator_instance_is_rejected() {
    let temp_dir =
        std::env::temp_dir().join(format!("agentxflow_db_lock_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let db_path = temp_dir.join("test.db");
    let lock_path = db_path.with_extension("lock");

    let first = DbPool::new(&db_path).expect("First coordinator instance must acquire the lock");
    let second = DbPool::new(&db_path);
    let err = second.expect_err("A second coordinator instance must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("already running") || msg.contains("lock"),
        "unexpected error message: {}",
        msg
    );

    drop(first);
    std::fs::remove_file(&db_path).ok();
    std::fs::remove_file(&lock_path).ok();
    std::fs::remove_dir_all(&temp_dir).ok();
}
