use hermes_dash_core::{
    model::{group_by_column, Status, Task, COLUMNS},
    source::parse_list,
};

#[test]
fn groups_fixture_into_all_six_columns_in_order_with_stable_task_order() {
    assert_eq!(
        COLUMNS,
        [
            Status::Todo,
            Status::Ready,
            Status::Running,
            Status::Blocked,
            Status::Review,
            Status::Done
        ]
    );
    let tasks = parse_list(include_str!("fixtures/list_sample.json")).unwrap();
    let columns = group_by_column(&tasks);
    assert_eq!(
        columns.iter().map(|c| c.status).collect::<Vec<_>>(),
        COLUMNS
    );
    assert_eq!(
        columns.iter().map(|c| c.tasks.len()).collect::<Vec<_>>(),
        [2, 0, 1, 0, 0, 1]
    );
    assert_eq!(
        columns[0]
            .tasks
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["t_todo_1", "t_todo_2"]
    );
    let wire = serde_json::to_value(&columns).unwrap();
    assert_eq!(wire[0]["status"], "todo");
    assert_eq!(wire[2]["tasks"][0]["assignee"], serde_json::Value::Null);
}

#[test]
fn every_column_sorts_cards_by_latest_update_first() {
    let tasks: Vec<Task> = COLUMNS
        .into_iter()
        .flat_map(|status| {
            [("old", 100), ("newest", 300), ("middle", 200)]
                .into_iter()
                .map(move |(id, last_updated)| Task {
                    id: id.into(),
                    title: id.into(),
                    status,
                    assignee: None,
                    created_at: None,
                    started_at: None,
                    completed_at: None,
                    last_updated: Some(last_updated),
                    last_updated_source: None,
                })
        })
        .collect();
    let original = tasks.clone();
    for column in group_by_column(&tasks) {
        assert_eq!(
            column.tasks.iter().map(|task| task.id.as_str()).collect::<Vec<_>>(),
            ["newest", "middle", "old"],
            "wrong order for {:?}", column.status
        );
    }
    assert_eq!(tasks, original, "grouping must not mutate the source tasks");
}

#[test]
fn equal_and_missing_updates_keep_source_order_with_missing_values_last() {
    let tasks: Vec<_> = [
        ("missing_first", None),
        ("equal_first", Some(300)),
        ("oldest", Some(i64::MIN)),
        ("missing_second", None),
        ("equal_second", Some(300)),
        ("newest", Some(i64::MAX)),
        ("epoch", Some(0)),
    ]
    .into_iter()
    .map(|(id, last_updated)| Task {
        id: id.into(),
        title: id.into(),
        status: Status::Todo,
        assignee: None,
        created_at: None,
        started_at: None,
        completed_at: None,
        last_updated,
        last_updated_source: None,
    })
    .collect();
    let columns = group_by_column(&tasks);
    assert_eq!(
        columns[0].tasks.iter().map(|task| task.id.as_str()).collect::<Vec<_>>(),
        ["newest", "equal_first", "equal_second", "epoch", "oldest", "missing_first", "missing_second"]
    );
}

#[test]
fn empty_input_keeps_six_empty_columns() {
    let columns = group_by_column(&[]);
    assert_eq!(columns.len(), 6);
    assert!(columns.iter().all(|c| c.tasks.is_empty()));
}

#[test]
fn hidden_statuses_are_dropped() {
    let tasks: Vec<_> = [
        Status::Triage,
        Status::Scheduled,
        Status::Archived,
        Status::Unknown,
    ]
    .into_iter()
    .map(|status| Task {
        id: "hidden".into(),
        title: "Hidden".into(),
        status,
        assignee: None, created_at: None, started_at: None, completed_at: None, last_updated: None, last_updated_source: None })
    .collect();
    let columns = group_by_column(&tasks);
    assert_eq!(columns.len(), 6);
    assert!(columns.iter().all(|c| c.tasks.is_empty()));
}

#[test]
fn every_visible_status_has_its_own_column() {
    let tasks: Vec<_> = COLUMNS
        .into_iter()
        .rev()
        .map(|status| Task {
            id: format!("{status:?}"),
            title: "Visible".into(),
            status,
            assignee: None,
            created_at: None,
            started_at: None,
            completed_at: None,
            last_updated: None,
            last_updated_source: None,
        })
        .collect();
    for (column, status) in group_by_column(&tasks).iter().zip(COLUMNS) {
        assert_eq!(column.status, status);
        assert_eq!(column.tasks.len(), 1);
        assert_eq!(column.tasks[0].status, status);
    }
}
