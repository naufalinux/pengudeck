use hermes_dash_core::model::{Status, Task};

#[test]
fn task_roundtrips_only_the_four_model_fields() {
    let task: Task = serde_json::from_str(
        r#"{"id":"t_one","title":"One","status":"todo","assignee":"archi","created_by":"zaha"}"#,
    )
    .unwrap();
    assert_eq!(task.id, "t_one");
    assert_eq!(task.title, "One");
    assert_eq!(task.status, Status::Todo);
    assert_eq!(task.assignee.as_deref(), Some("archi"));
    assert_eq!(
        serde_json::to_value(task).unwrap(),
        serde_json::json!({
            "id":"t_one", "title":"One", "status":"todo", "assignee":"archi"
        })
    );
}

#[test]
fn every_known_status_uses_lowercase_and_new_status_is_unknown() {
    for (wire, status) in [
        ("triage", Status::Triage),
        ("todo", Status::Todo),
        ("scheduled", Status::Scheduled),
        ("ready", Status::Ready),
        ("running", Status::Running),
        ("blocked", Status::Blocked),
        ("review", Status::Review),
        ("done", Status::Done),
        ("archived", Status::Archived),
    ] {
        assert_eq!(
            serde_json::from_value::<Status>(serde_json::json!(wire)).unwrap(),
            status
        );
        assert_eq!(serde_json::to_value(status).unwrap(), wire);
    }
    assert_eq!(
        serde_json::from_str::<Status>("\"future_status\"").unwrap(),
        Status::Unknown
    );
}
