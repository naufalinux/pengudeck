use dioxus::prelude::*;
use hermes_dash_core::model::{group_by_column, Task};

pub const API_BASE: &str = "http://127.0.0.1:8788";

fn tasks_url(slug: &str) -> Option<String> {
    (!slug.is_empty()
        && slug
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
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

#[component]
fn BoardPicker(boards: Vec<String>, selected: String, on_select: EventHandler<String>) -> Element {
    rsx! {
        label { r#for: "board", "Board" }
        select {
            id: "board", value: "{selected}", disabled: boards.is_empty(),
            onchange: move |event| on_select.call(event.value()),
            option { value: "", disabled: true, "Pilih board" }
            for slug in &boards {
                option { key: "{slug}", value: "{slug}", "{slug}" }
            }
        }
    }
}

async fn request(url: String, kind: &str) -> Result<(Vec<String>, Vec<Task>), &'static str> {
    let eval = document::eval(include_str!("fetch.js"));
    eval.send((url, kind))
        .map_err(|_| "API tidak dapat dihubungi.")?;
    let (status, boards, tasks): (u16, Vec<String>, Vec<Task>) =
        eval.join().await.map_err(|_| "Respons API tidak valid.")?;
    match response_error(status) {
        Some(message) => Err(message),
        None => Ok((boards, tasks)),
    }
}

#[component]
fn App() -> Element {
    let mut selected = use_signal(String::new);
    let mut boards = use_resource(|| async {
        request(format!("{API_BASE}/api/boards"), "boards")
            .await
            .map(|data| data.0)
    });
    // Reactive resource cancels its previous future on selection changes.
    // Tag the result too: never render a previous board under the new selector.
    let mut cards = use_resource(move || async move {
        let slug = selected();
        let result = match tasks_url(&slug) {
            Some(url) => request(url, "tasks").await.map(|data| data.1),
            None => Ok(Vec::new()),
        };
        (slug, result)
    });
    let board_state = boards.read();
    let slugs = match board_state.as_ref() {
        Some(Ok(slugs)) => slugs.clone(),
        _ => Vec::new(),
    };
    let card_state = cards.read();
    let mut tasks = Vec::new();
    let mut card_error = None;
    let mut loading = !selected().is_empty();
    if let Some((slug, result)) = card_state.as_ref() {
        if *slug == selected() {
            loading = false;
            match result {
                Ok(data) => tasks = data.clone(),
                Err(message) => card_error = Some(*message),
            }
        }
    }
    rsx! {
        style { {include_str!("style.css")} }
        main {
            header {
                h1 { "Hermes Kanban" }
                BoardPicker { boards: slugs, selected: selected(),
                    on_select: move |slug| selected.set(slug) }
                button { onclick: move |_| { boards.restart(); cards.restart(); }, "Muat ulang" }
            }
            div { role: "status", "aria-live": "polite",
                if board_state.is_none() { p { "Memuat daftar board…" } }
                if let Some(Ok(slugs)) = board_state.as_ref() {
                    if slugs.is_empty() { p { "Tidak ada board." } }
                    else if selected().is_empty() { p { "Pilih board untuk melihat kartu." } }
                }
                if loading { p { "Memuat kartu…" } }
            }
            if let Some(Err(message)) = board_state.as_ref() { p { role: "alert", "{message}" } }
            if let Some(message) = card_error { p { role: "alert", "{message}" } }
            BoardView { tasks }
        }
    }
}

fn main() {
    dioxus::launch(App);
}

#[cfg(test)]
mod tests {
    #[test]
    fn initial_app_mounts_without_assuming_a_board() {
        use dioxus::prelude::*;
        let mut dom = VirtualDom::new(super::App);
        let mutations = format!("{:?}", dom.rebuild_to_vec());
        assert!(mutations.contains("Bool(true)")); // empty selector is disabled
        for label in ["Todo", "Ready", "Running", "Blocked", "Review", "Done"] {
            assert!(mutations.contains(label));
        }
        assert!(!mutations.contains("team-a"));
    }

    #[test]
    fn selector_renders_api_slugs_and_current_selection() {
        use dioxus::prelude::*;
        let mut dom = VirtualDom::new(|| {
            rsx! {
                super::BoardPicker {
                    boards: vec!["team-a".into(), "team_b".into()],
                    selected: "team_b".to_string(),
                    on_select: |_| {},
                }
            }
        });
        let mutations = format!("{:?}", dom.rebuild_to_vec());
        for text in ["change", "team-a", "team_b", "Bool(false)"] {
            assert!(mutations.contains(text), "missing {text}: {mutations}");
        }
    }

    #[test]
    fn board_renders_six_columns_and_card_fields_as_text() {
        use dioxus::prelude::*;
        use hermes_dash_core::model::{Status, Task};
        let mut dom = VirtualDom::new(|| {
            rsx! {
                super::BoardView { tasks: vec![Task {
                    id: "t_visible".into(), title: "<script>alert(1)</script>".into(),
                    status: Status::Running, assignee: Some("archi".into()),
                }, Task {
                    id: "t_unassigned".into(), title: "Unassigned".into(),
                    status: Status::Todo, assignee: None,
                }] }
            }
        });
        let mutations = format!("{:?}", dom.rebuild_to_vec());
        for text in [
            "Todo",
            "Ready",
            "Running",
            "Blocked",
            "Review",
            "Done",
            "t_visible",
            "<script>alert(1)</script>",
            "archi",
            "Belum ditugaskan",
        ] {
            assert!(mutations.contains(text), "missing {text}: {mutations}");
        }
    }

    #[test]
    fn failures_are_short_static_messages() {
        assert_eq!(super::response_error(200), None);
        assert_eq!(super::response_error(0), Some("API tidak dapat dihubungi."));
        assert_eq!(super::response_error(1), Some("Respons API tidak valid."));
        for status in [201, 204, 301, 400, 404, 500, 502] {
            assert_eq!(
                super::response_error(status),
                Some("API gagal memuat data.")
            );
        }
    }

    #[test]
    fn requests_use_fixed_loopback_and_only_valid_slugs() {
        assert_eq!(super::API_BASE, "http://127.0.0.1:8788");
        assert_eq!(
            super::tasks_url("team-A_1"),
            Some("http://127.0.0.1:8788/api/boards/team-A_1/tasks".into())
        );
        for slug in ["", "../secrets", "a/b", "a?b", "a\"b", "é", "a b"] {
            assert_eq!(super::tasks_url(slug), None);
        }
    }
}
