use dioxus::prelude::*;
use hermes_dash_core::model::{group_by_column, Task};

pub const API_BASE: &str = "http://127.0.0.1:8787";

fn tasks_url(slug: &str) -> Option<String> {
    (!slug.is_empty() && slug.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
        .then(|| format!("{API_BASE}/api/boards/{slug}/tasks"))
}

fn response_error(status: u16) -> Option<&'static str> {
    match status {
        200 => None,
        0 => Some("API tidak dapat dihubungi."),
        1 => Some("Respons API tidak valid."),
        _ => Some("API gagal memuat data."),
    }
}

#[component]
fn BoardView(tasks: Vec<Task>) -> Element {
    let columns = group_by_column(&tasks);
    let labels = ["Todo", "Ready", "Running", "Blocked", "Review", "Done"];
    rsx! {
        section { class: "board", "aria-label": "Kolom Kanban",
            for (column, label) in columns.iter().zip(labels) {
                section { class: "column", key: "{label}",
                    h2 { "{label}" }
                    if column.tasks.is_empty() { p { class: "empty", "Tidak ada kartu." } }
                    for task in &column.tasks {
                        article { class: "card", key: "{task.id}",
                            code { "{task.id}" }
                            h3 { "{task.title}" }
                            p { "{task.assignee.as_deref().unwrap_or(\"Belum ditugaskan\")}" }
                        }
                    }
                }
            }
        }
    }
}

fn main() {}

#[cfg(test)]
mod tests {
    #[test]
    fn board_renders_six_columns_and_card_fields_as_text() {
        use dioxus::prelude::*;
        use hermes_dash_core::model::{Status, Task};
        let mut dom = VirtualDom::new(|| rsx! {
            super::BoardView { tasks: vec![Task {
                id: "t_visible".into(), title: "<script>alert(1)</script>".into(),
                status: Status::Running, assignee: Some("archi".into()),
            }, Task {
                id: "t_unassigned".into(), title: "Unassigned".into(),
                status: Status::Todo, assignee: None,
            }] }
        });
        let mutations = format!("{:?}", dom.rebuild_to_vec());
        for text in ["Todo", "Ready", "Running", "Blocked", "Review", "Done",
            "t_visible", "<script>alert(1)</script>", "archi", "Belum ditugaskan"] {
            assert!(mutations.contains(text), "missing {text}: {mutations}");
        }
    }

    #[test]
    fn failures_are_short_static_messages() {
        assert_eq!(super::response_error(200), None);
        assert_eq!(super::response_error(0), Some("API tidak dapat dihubungi."));
        assert_eq!(super::response_error(1), Some("Respons API tidak valid."));
        for status in [201, 204, 301, 400, 404, 500, 502] {
            assert_eq!(super::response_error(status), Some("API gagal memuat data."));
        }
    }

    #[test]
    fn requests_use_fixed_loopback_and_only_valid_slugs() {
        assert_eq!(super::API_BASE, "http://127.0.0.1:8787");
        assert_eq!(super::tasks_url("team-A_1"), Some("http://127.0.0.1:8787/api/boards/team-A_1/tasks".into()));
        for slug in ["", "../secrets", "a/b", "a?b", "a\"b", "é", "a b"] {
            assert_eq!(super::tasks_url(slug), None);
        }
    }
}
