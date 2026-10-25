use hermes_dash_core::{model::Status, source::parse_list};

#[test]
fn parses_five_task_fixture_in_source_order() {
    let tasks = parse_list(include_str!("fixtures/list_sample.json")).unwrap();
    assert_eq!(tasks.len(), 5);
    assert_eq!(
        tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        ["t_todo_1", "t_running", "t_todo_2", "t_done", "t_future"]
    );
    assert_eq!(tasks[0].assignee.as_deref(), Some("archi"));
    assert_eq!(tasks[1].assignee, None);
    assert_eq!(tasks[4].status, Status::Unknown);
}

#[test]
fn rejects_invalid_json_without_echoing_input() {
    let error = parse_list("SECRET_PAYLOAD not json").unwrap_err();
    assert!(!format!("{error} {error:?}").contains("SECRET_PAYLOAD"));
}

#[test]
fn rejects_non_arrays_and_invalid_elements() {
    for input in ["{}", "null", "42", "\"text\"", "[null]", "[42]"] {
        assert!(parse_list(input).is_err(), "accepted {input}");
    }
}

#[test]
fn rejects_missing_required_fields_and_wrong_model_types() {
    let valid = serde_json::json!({"id":"t_one","title":"One","status":"todo","assignee":"archi"});
    for field in ["id", "title", "status"] {
        let mut task = valid.clone();
        task.as_object_mut().unwrap().remove(field);
        assert!(parse_list(&serde_json::json!([task]).to_string()).is_err());
    }
    for field in ["id", "title", "status", "assignee"] {
        for invalid in [
            serde_json::json!(42),
            serde_json::json!({}),
            serde_json::json!([]),
        ] {
            let mut task = valid.clone();
            task[field] = invalid;
            assert!(parse_list(&serde_json::json!([task]).to_string()).is_err());
        }
    }
}

#[test]
fn missing_or_null_assignee_is_none() {
    let tasks = parse_list(r#"[{"id":"one","title":"One","status":"todo"},{"id":"two","title":"Two","status":"done","assignee":null}]"#).unwrap();
    assert!(tasks.iter().all(|t| t.assignee.is_none()));
}

#[test]
fn empty_array_is_successful() {
    assert!(parse_list("[]").unwrap().is_empty());
}
