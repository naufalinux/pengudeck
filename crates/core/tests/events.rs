use hermes_dash_core::{
    events::{board_db_path, enrich, latest_event_times},
    model::{LastUpdatedSource, Status, Task},
    source::SourceError,
};
use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("hdash-events-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn create_board_db(&self, board: &str, events: &[(&str, i64)]) -> PathBuf {
        let path = board_db_path(&self.0, board).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE task_events (id INTEGER PRIMARY KEY, task_id TEXT NOT NULL, \
             run_id INTEGER, kind TEXT NOT NULL, payload TEXT, created_at INTEGER NOT NULL);",
        )
        .unwrap();
        for (task_id, ts) in events {
            conn.execute(
                "INSERT INTO task_events (task_id, kind, created_at) VALUES (?1, 'x', ?2)",
                (task_id, *ts),
            )
            .unwrap();
        }
        path
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn task(id: &str, created: i64, started: Option<i64>, completed: Option<i64>) -> Task {
    Task {
        id: id.into(),
        title: id.into(),
        status: Status::Done,
        assignee: None,
        created_at: Some(created),
        started_at: started,
        completed_at: completed,
        last_updated: None,
        last_updated_source: None,
    }
}

#[test]
fn default_board_uses_root_db_and_others_use_board_dir() {
    let home = Path::new("/h/.hermes");
    assert_eq!(board_db_path(home, "default").unwrap(), home.join("kanban.db"));
    assert_eq!(
        board_db_path(home, "zaha").unwrap(),
        home.join("kanban/boards/zaha/kanban.db")
    );
}

#[test]
fn rejects_slugs_that_could_escape_the_boards_dir() {
    let home = Path::new("/h/.hermes");
    for bad in ["", "../x", "a/b", "a b", "a.b", "..", "x\0"] {
        assert!(board_db_path(home, bad).is_none(), "{bad:?} must be rejected");
    }
}

#[test]
fn latest_event_per_task_is_read_from_read_only_db() {
    let home = Home::new("latest");
    let db = home.create_board_db("zaha", &[("t_1", 100), ("t_1", 300), ("t_1", 200), ("t_2", 50)]);
    let map = latest_event_times(&db).unwrap();
    assert_eq!(map.get("t_1"), Some(&300));
    assert_eq!(map.get("t_2"), Some(&50));
}

#[test]
fn enrich_prefers_event_and_never_goes_older_than_timestamps() {
    let home = Home::new("enrich");
    home.create_board_db("zaha", &[("t_1", 500), ("t_2", 10)]);
    let mut tasks = vec![
        task("t_1", 100, Some(200), Some(300)), // event 500 is newest
        task("t_2", 100, None, Some(900)),      // timestamp 900 newer than event 10
        task("t_3", 400, None, None),           // no event rows: fallback
    ];
    enrich(&mut tasks, "zaha", &home.0);
    assert_eq!(tasks[0].last_updated, Some(500));
    assert_eq!(tasks[0].last_updated_source, Some(LastUpdatedSource::Events));
    assert_eq!(tasks[1].last_updated, Some(900));
    assert_eq!(tasks[1].last_updated_source, Some(LastUpdatedSource::Timestamps));
    assert_eq!(tasks[2].last_updated, Some(400));
    assert_eq!(tasks[2].last_updated_source, Some(LastUpdatedSource::Timestamps));
}

#[test]
fn missing_db_falls_back_without_creating_a_file() {
    let home = Home::new("missing");
    let mut tasks = vec![task("t_1", 100, None, None)];
    enrich(&mut tasks, "ghost", &home.0);
    assert_eq!(tasks[0].last_updated, Some(100));
    let db = board_db_path(&home.0, "ghost").unwrap();
    assert!(!db.exists(), "read-only open must not create the database");
    assert_eq!(latest_event_times(&db), Err(SourceError::Unavailable));
}

#[test]
fn read_only_connection_rejects_writes() {
    let home = Home::new("ro");
    let db = home.create_board_db("zaha", &[("t_1", 1)]);
    let conn = rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert!(conn.execute("DELETE FROM task_events", []).is_err());
}
