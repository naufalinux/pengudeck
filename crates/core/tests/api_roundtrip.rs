use hermes_dash_core::model::{LastUpdatedSource, Task};

#[test]
fn api_json_keeps_last_updated_when_web_reads_it() {
    let json = r#"{"id":"t_1","title":"T","status":"done","assignee":null,
        "last_updated":1792963304,"last_updated_source":"events"}"#;
    let task: Task = serde_json::from_str(json).unwrap();
    assert_eq!(task.last_updated, Some(1792963304));
    assert_eq!(task.last_updated_source, Some(LastUpdatedSource::Events));
}
