use dioxus::prelude::*;
use hermes_dash_core::model::{group_by_column, LastUpdatedSource, Task};
mod settings;

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

/// Formats unix seconds as `YYYY-MM-DD HH:MM WIB` (Asia/Jakarta, UTC+7, no DST).
pub fn format_last_updated(ts: i64) -> String {
    let secs = ts + 7 * 3600;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} WIB", rem / 3600, (rem % 3600) / 60)
}

/// Howard Hinnant's days-to-civil conversion (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Label for the card line. Says where the value came from, so a fallback is not mistaken for an event.
fn last_updated_label(task: &Task) -> Option<String> {
    let ts = task.last_updated?;
    let when = format_last_updated(ts);
    Some(match task.last_updated_source {
        Some(LastUpdatedSource::Events) => format!("Terakhir diperbarui: {when}"),
        _ => format!("Terakhir diperbarui: {when} (dari created/started/completed)"),
    })
}

#[component]
fn BoardView(tasks: Vec<Task>) -> Element {
    let columns = group_by_column(&tasks);
    let labels = [
        ("Todo", "todo"),
        ("Ready", "ready"),
        ("Running", "running"),
        ("Blocked", "blocked"),
        ("Review", "review"),
        ("Done", "done"),
    ];
    rsx! {
        section { class: "board", "aria-label": "Kolom Kanban",
            for (column, (label, status)) in columns.iter().zip(labels) {
                section { class: "column", key: "{label}", "data-status": "{status}",
                    h2 { "{label}" }
                    if column.tasks.is_empty() { p { class: "empty", "Tidak ada kartu." } }
                    for task in &column.tasks {
                        article { class: "card", key: "{task.id}",
                            code { "{task.id}" }
                            h3 { "{task.title}" }
                            p { "{task.assignee.as_deref().unwrap_or(\"Belum ditugaskan\")}" }
                            if let Some(label) = last_updated_label(task) { small { "{label}" } }
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

/// Chooses the board to open: the saved one if the API still lists it, else `default`, else nothing.
pub fn choose_initial_board(saved: &str, slugs: &[String]) -> Option<String> {
    if !saved.is_empty() && slugs.iter().any(|s| s == saved) {
        return Some(saved.to_string());
    }
    slugs.iter().find(|s| *s == "default").cloned()
}

async fn read_saved_board() -> String {
    let eval = document::eval(include_str!("storage_get.js"));
    eval.join::<String>().await.unwrap_or_default()
}

fn save_board(slug: &str) {
    if tasks_url(slug).is_some() {
        let eval = document::eval(include_str!("storage_set.js"));
        let _ = eval.send(slug.to_string());
    }
}

/// Only "dark" selects dark mode. Anything else, including nothing saved, is light (the site default).
pub fn normalize_theme(saved: &str) -> &'static str {
    if saved == "dark" { "dark" } else { "light" }
}

async fn read_saved_theme() -> String {
    let eval = document::eval(include_str!("theme_get.js"));
    eval.join::<String>().await.unwrap_or_default()
}

/// Applies the theme to <html> so body and page background pick up the same tokens as the cards.
fn apply_theme(theme: &str) {
    let eval = document::eval(include_str!("theme_apply.js"));
    let _ = eval.send(theme.to_string());
}

fn save_theme(theme: &str) {
    let eval = document::eval(include_str!("theme_set.js"));
    let _ = eval.send(theme.to_string());
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
    let mut show_settings = use_signal(|| false);
    let mut selected = use_signal(String::new);
    let mut boards = use_resource(|| async {
        request(format!("{API_BASE}/api/boards"), "boards")
            .await
            .map(|data| data.0)
    });
    let saved = use_resource(read_saved_board);
    // Set once: either the first board list is applied, or the user has picked a board.
    let mut chosen = use_signal(|| false);
    use_effect(move || {
        if chosen() {
            return;
        }
        let saved_slug = saved.read().as_ref().cloned();
        let listed: Option<Result<Vec<String>, &'static str>> = boards.read().as_ref().cloned();
        if let (Some(saved_slug), Some(Ok(slugs))) = (saved_slug, listed) {
            chosen.set(true);
            if let Some(pick) = choose_initial_board(&saved_slug, &slugs) {
                selected.set(pick);
            }
        }
    });
    let saved_theme = use_resource(read_saved_theme);
    let mut theme = use_signal(|| "light".to_string());
    let mut theme_chosen = use_signal(|| false);
    use_effect(move || {
        if theme_chosen() {
            return;
        }
        if let Some(saved_value) = saved_theme.read().as_ref().cloned() {
            theme_chosen.set(true);
            theme.set(normalize_theme(&saved_value).to_string());
        }
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
    use_effect(move || apply_theme(&theme()));
    let theme_label = if theme() == "dark" { "Mode terang" } else { "Mode gelap" };
    rsx! {
        style { {include_str!("style.css")} }
        main { "data-theme": "{theme()}",
            header {
                div { class: "brand", "One Penguin Tech" }
                h1 { "Hermes Kanban" }
                div { class: "controls",
                    button { class: "btn", "aria-expanded": show_settings(),
                        onclick: move |_| show_settings.set(!show_settings()),
                        if show_settings() { "Kembali ke board" } else { "Settings" } }
                    BoardPicker { boards: slugs.clone(), selected: selected(),
                        on_select: move |slug: String| {
                            chosen.set(true);
                            save_board(&slug);
                            selected.set(slug);
                        } }
                    button { class: "btn", onclick: move |_| { boards.restart(); cards.restart(); }, "Muat ulang" }
                    button { class: "btn",
                        onclick: move |_| {
                            let next = if theme() == "dark" { "light" } else { "dark" };
                            theme_chosen.set(true);
                            save_theme(next);
                            theme.set(next.to_string());
                        },
                        "{theme_label}" }
                }
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
            if show_settings() { settings::CleanupPanel { boards: slugs.clone() } }
            else { BoardView { tasks } }
        }
    }
}

fn main() {
    dioxus::launch(App);
}

#[cfg(test)]
mod tests {
    use hermes_dash_core::model::{LastUpdatedSource, Status, Task};

    #[test]
    fn initial_board_prefers_saved_then_default_then_nothing() {
        let slugs = vec!["reporty".to_string(), "default".to_string(), "zaha".to_string()];
        assert_eq!(super::choose_initial_board("zaha", &slugs), Some("zaha".into()));
        // Saved board removed from the API: fall back to default.
        assert_eq!(super::choose_initial_board("gone", &slugs), Some("default".into()));
        assert_eq!(super::choose_initial_board("", &slugs), Some("default".into()));
        let no_default = vec!["reporty".to_string()];
        assert_eq!(super::choose_initial_board("", &no_default), None);
        assert_eq!(super::choose_initial_board("reporty", &no_default), Some("reporty".into()));
        assert_eq!(super::choose_initial_board("default", &[]), None);
    }
    #[test]
    fn theme_is_dark_only_when_saved_as_dark() {
        assert_eq!(super::normalize_theme("dark"), "dark");
        assert_eq!(super::normalize_theme("light"), "light");
        assert_eq!(super::normalize_theme(""), "light");
        assert_eq!(super::normalize_theme("javascript:alert(1)"), "light");
    }

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
        let mut dom = VirtualDom::new(|| {
            rsx! {
                super::BoardView { tasks: vec![Task {
                    id: "t_visible".into(), title: "<script>alert(1)</script>".into(),
                    status: Status::Running, assignee: Some("archi".into()), created_at: None, started_at: None, completed_at: None, last_updated: None, last_updated_source: None }, Task {
                    id: "t_unassigned".into(), title: "Unassigned".into(),
                    status: Status::Todo, assignee: None, created_at: None, started_at: None, completed_at: None, last_updated: None, last_updated_source: None }] }
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
    fn board_renders_newest_updated_card_before_older_and_unknown_cards() {
        use dioxus::prelude::*;
        let mut dom = VirtualDom::new(|| {
            let tasks = [("unknown_card", None), ("older_card", Some(100)), ("newest_card", Some(300))]
                .into_iter()
                .map(|(id, last_updated)| Task {
                    id: id.into(), title: "Card".into(), status: Status::Todo,
                    assignee: None, created_at: None, started_at: None, completed_at: None,
                    last_updated, last_updated_source: None,
                })
                .collect::<Vec<_>>();
            rsx! { super::BoardView { tasks } }
        });
        let mutations = format!("{:?}", dom.rebuild_to_vec());
        let newest = mutations.find("newest_card").expect("newest card rendered");
        let older = mutations.find("older_card").expect("older card rendered");
        let unknown = mutations.find("unknown_card").expect("unknown card rendered");
        assert!(newest < older && older < unknown, "wrong card order: {mutations}");
    }

    #[test]
    fn formats_utc8_dates_and_known_epochs() {
        assert_eq!(super::format_last_updated(0), "1970-01-01 07:00 WIB");
        // 1792957202 is created_at of task t_437645c2: 2026-10-26 03:40 UTC+8 = 02:40 WIB.
        assert_eq!(super::format_last_updated(1_792_957_202), "2026-10-26 02:40 WIB");
        assert_eq!(super::format_last_updated(951_782_400), "2000-02-29 07:00 WIB");
    }

    #[test]
    fn label_names_the_source_and_is_absent_without_value() {
        let mut t = Task {
            id: "t".into(), title: "t".into(), status: Status::Done, assignee: None,
            created_at: None, started_at: None, completed_at: None,
            last_updated: None, last_updated_source: None,
        };
        assert_eq!(super::last_updated_label(&t), None);
        t.last_updated = Some(0);
        t.last_updated_source = Some(LastUpdatedSource::Events);
        assert_eq!(super::last_updated_label(&t).unwrap(), "Terakhir diperbarui: 1970-01-01 07:00 WIB");
        t.last_updated_source = Some(LastUpdatedSource::Timestamps);
        assert!(super::last_updated_label(&t).unwrap().ends_with("(dari created/started/completed)"));
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
