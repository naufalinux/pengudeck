use dioxus::prelude::*;
use serde_json::{json, Value};

const FAILED: &str = "Belum bisa dipastikan. Muat ulang halaman ini untuk melihat status terbaru.";

/// Builds the policy sent to the server from plain form values.
pub fn policy_from_form(
    enabled: bool,
    archive: &str,
    purge: &str,
    selected: &[String],
) -> Option<Value> {
    let archive: u32 = archive.trim().parse().ok()?;
    let purge: u32 = purge.trim().parse().ok()?;
    if !(1..=3650).contains(&archive) || !(1..=3650).contains(&purge) || selected.is_empty() {
        return None;
    }
    let mut boards = selected.to_vec();
    boards.sort();
    boards.dedup();
    Some(json!({"enabled":enabled,"archive_days":archive,"purge_days":purge,"boards":boards}))
}

fn normalized(policy: &Value) -> Value {
    let mut policy = policy.clone();
    if let Some(boards) = policy["boards"].as_array_mut() {
        boards.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    }
    policy
}

pub fn error_message(code: &str) -> &'static str {
    match code {
        "confirmation_required" => "Konfirmasi diperlukan sebelum menyimpan.",
        "retention_busy" => "Pembersihan sedang berjalan. Coba lagi sebentar lagi.",
        "stale_preview" => {
            "Isi board berubah sejak dihitung. Muat ulang halaman ini lalu coba lagi."
        }
        "invalid_days" | "invalid_policy" | "invalid_enabled" => "Isi hari antara 1 dan 3650.",
        "invalid_boards" | "invalid_board" => "Pilih minimal satu board yang valid.",
        "backup_timeout" | "backup_failed" => {
            "Backup database gagal, jadi tidak ada card yang diubah. Pembersihan otomatis dimatikan sampai berhasil disimpan."
        }
        "incomplete_tracking_schema" | "unsupported_tracking_version" => {
            "Board punya pencatatan yang tidak dikenali. Tidak ada card yang diubah."
        }
        _ => FAILED,
    }
}

fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn summary_text(summary: &Value) -> String {
    if summary.is_null() {
        return "Jumlah card belum bisa dihitung.".into();
    }
    let untracked = names(&summary["untracked"]);
    if !untracked.is_empty() {
        return format!(
            "Umur card baru mulai dihitung setelah pembersihan diaktifkan (board: {}).",
            untracked.join(", ")
        );
    }
    let (archive, purge) = (
        summary["archive"].as_u64().unwrap_or(0),
        summary["purge"].as_u64().unwrap_or(0),
    );
    if archive + purge == 0 {
        "Saat ini tidak ada card yang perlu dibersihkan.".into()
    } else {
        format!("Saat ini {archive} card akan diarsipkan dan {purge} card akan dihapus permanen.")
    }
}

pub fn last_run_text(last_run: &Value) -> String {
    let Some(at) = last_run["at"].as_i64() else {
        return "Belum pernah berjalan.".into();
    };
    let count = |key: &str| last_run[key].as_array().map_or(0, Vec::len);
    let mut text = format!(
        "{}: {} diarsipkan, {} dihapus permanen.",
        crate::format_last_updated(at),
        count("archived"),
        count("purged")
    );
    if count("errors") > 0 {
        text.push_str(" Ada error; sebagian card mungkin belum diproses.");
    }
    text
}

async fn request(operation: &str, payload: Value) -> Result<Value, &'static str> {
    let eval = document::eval(include_str!("cleanup.js"));
    eval.send((operation.to_string(), payload))
        .map_err(|_| FAILED)?;
    let response: String = eval.join().await.map_err(|_| FAILED)?;
    let data: Value = serde_json::from_str(&response).map_err(|_| FAILED)?;
    if data["ok"] == true {
        Ok(data["data"].clone())
    } else {
        Err(error_message(data["code"].as_str().unwrap_or_default()))
    }
}

#[derive(Clone, PartialEq)]
struct Confirm {
    run_now: bool,
    /// Exactly what the user reviewed; confirming sends this snapshot, never live form values.
    policy: Value,
    summary: Value,
    fingerprint: String,
}

async fn refresh(mut summary: Signal<Value>, mut fingerprint: Signal<String>) {
    // A failed refresh must not leave counts for an older policy that could be run.
    match request("status", json!({})).await {
        Ok(status) => {
            summary.set(status["summary"].clone());
            fingerprint.set(status["fingerprint"].as_str().unwrap_or_default().into());
        }
        Err(_) => {
            summary.set(Value::Null);
            fingerprint.set(String::new());
        }
    }
}

#[component]
pub fn CleanupPanel(boards: Vec<String>) -> Element {
    let mut data = use_resource(|| request("status", json!({})));
    rsx! {
        section { class: "settings-panel", "aria-label": "Pembersihan card otomatis",
            h2 { "Pembersihan card otomatis" }
            match data.read().as_ref() {
                None => rsx! { p { role: "status", "Memuat pengaturan…" } },
                Some(Err(message)) => rsx! {
                    p { role: "alert", "{message}" }
                    button { class: "btn", onclick: move |_| data.restart(), "Coba lagi" }
                },
                Some(Ok(value)) => rsx! { CleanupForm { key: "{value}", initial: value.clone(), boards: boards.clone() } },
            }
        }
    }
}

#[component]
fn CleanupForm(initial: Value, boards: Vec<String>) -> Element {
    let init_policy = initial["policy"].clone();
    let init_summary = initial["summary"].clone();
    let init_fingerprint = initial["fingerprint"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let init_last_run = initial["last_run"].clone();
    let init_boards = names(&init_policy["boards"]);
    let mut all_boards = boards.clone();
    for board in &init_boards {
        if !all_boards.contains(board) {
            all_boards.push(board.clone());
        }
    }
    let init_enabled = init_policy["enabled"].as_bool().unwrap_or(false);
    let init_archive = init_policy["archive_days"].to_string();
    let init_purge = init_policy["purge_days"].to_string();
    let mut saved = use_signal(move || init_policy);
    let mut enabled = use_signal(move || init_enabled);
    let mut archive = use_signal(move || init_archive);
    let mut purge = use_signal(move || init_purge);
    let mut selected = use_signal(move || init_boards);
    let summary = use_signal(move || init_summary);
    let fingerprint = use_signal(move || init_fingerprint);
    let mut last_run = use_signal(move || init_last_run);
    let mut confirm = use_signal(|| None::<Confirm>);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);

    let policy = policy_from_form(enabled(), &archive(), &purge(), &selected());
    let dirty = policy
        .as_ref()
        .map(|p| normalized(p) != normalized(&saved()))
        .unwrap_or(true);
    let can_save = policy.is_some() && dirty && !busy();
    let current = summary();
    let can_run = !dirty
        && !busy()
        && current["untracked"].as_array().is_some_and(Vec::is_empty)
        && current["archive"].as_u64().unwrap_or(0) + current["purge"].as_u64().unwrap_or(0) > 0
        && !fingerprint().is_empty();
    let dialog = confirm();
    let (a_days, p_days) = {
        let shown = dialog.as_ref().map(|d| &d.policy).unwrap_or(&Value::Null);
        (
            shown["archive_days"].to_string(),
            shown["purge_days"].to_string(),
        )
    };
    let dialog_boards = dialog
        .as_ref()
        .map(|d| names(&d.policy["boards"]).join(", "))
        .unwrap_or_default();
    let dialog_has_untracked = dialog
        .as_ref()
        .is_some_and(|d| !names(&d.summary["untracked"]).is_empty());
    let dialog_summary = dialog
        .as_ref()
        .map(|d| summary_text(&d.summary))
        .unwrap_or_default();
    let dialog_run = dialog.as_ref().is_some_and(|d| d.run_now);

    rsx! {
        p { "Arsipkan card Done yang sudah lama, lalu hapus permanen arsip yang terlalu lama. Card yang belum selesai tidak disentuh." }
        fieldset { disabled: busy() || confirm().is_some(),
            legend { "Pengaturan" }
            label { r#for: "cleanup-enabled", class: "check-label",
                input { id: "cleanup-enabled", r#type: "checkbox", checked: enabled(),
                    onchange: move |event| enabled.set(event.checked()) }
                "Bersihkan card otomatis setiap hari"
            }
            div { class: "retention-fields",
                label { r#for: "cleanup-archive-days", "Arsipkan card Done setelah (hari)" }
                input { id: "cleanup-archive-days", r#type: "number", inputmode: "numeric", min: "1", max: "3650", value: archive(),
                    oninput: move |event| archive.set(event.value()) }
                label { r#for: "cleanup-purge-days", "Hapus permanen arsip setelah (hari)" }
                input { id: "cleanup-purge-days", r#type: "number", inputmode: "numeric", min: "1", max: "3650", value: purge(),
                    oninput: move |event| purge.set(event.value()) }
            }
            div { role: "group", "aria-labelledby": "cleanup-boards-title",
                p { id: "cleanup-boards-title", class: "field-title", "Board yang dibersihkan" }
                for board in all_boards.iter().cloned() {
                    label { key: "{board}", class: "check-label",
                        input { r#type: "checkbox", name: "board", value: "{board}", checked: selected().contains(&board),
                            onchange: {
                                let board = board.clone();
                                move |event| {
                                    let mut now = selected();
                                    now.retain(|b| b != &board);
                                    if event.checked() { now.push(board.clone()); }
                                    selected.set(now);
                                }
                            } }
                        "{board}"
                    }
                }
            }
            p { class: "notice", "{summary_text(&current)}" }
            if policy.is_none() { p { role: "alert", "Isi hari 1–3650 dan pilih minimal satu board." } }
            div { class: "settings-actions",
                button { class: "btn", id: "cleanup-save", disabled: !can_save,
                    onclick: move |_| {
                        let Some(policy) = policy.clone() else { return; };
                        busy.set(true); message.set(String::new());
                        spawn(async move {
                            if policy["enabled"] == false {
                                match request("save", json!({"policy":policy})).await {
                                    Ok(value) => { saved.set(value["policy"].clone()); message.set("Pembersihan otomatis dimatikan.".into()); }
                                    Err(error) => message.set(error.into()),
                                }
                            } else {
                                match request("review", json!({"policy":policy})).await {
                                    Ok(value) => confirm.set(Some(Confirm { run_now: false, policy: policy.clone(), summary: value["summary"].clone(), fingerprint: String::new() })),
                                    Err(error) => message.set(error.into()),
                                }
                            }
                            busy.set(false);
                        });
                    }, "Simpan" }
            }
        }
        p { role: "status", "aria-live": "polite", if busy() { "Memproses…" } else { "{message()}" } }
        details {
            summary { "Lanjutan" }
            p { "Riwayat: {last_run_text(&last_run())}" }
            p { class: "notice", "Berjalan otomatis maksimal sekali sehari selama Pengudeck hidup, paling banyak 50 card per proses. Aktivitas baru pada card menunda pembersihannya." }
            button { class: "btn", id: "cleanup-run", disabled: !can_run,
                onclick: move |_| confirm.set(Some(Confirm { run_now: true, policy: saved(), summary: summary(), fingerprint: fingerprint() })),
                "Jalankan sekarang" }
            if !can_run && !busy() { p { class: "hint", "Tombol aktif jika pengaturan sudah tersimpan dan ada card yang memenuhi syarat." } }
        }
        if let Some(dialog) = dialog {
            div { class: "dialog-backdrop",
                div { class: "dialog", role: "alertdialog", "aria-modal": "true",
                    "aria-labelledby": "dialog-title", "aria-describedby": "dialog-body",
                    onkeydown: move |event| if event.key() == Key::Escape { confirm.set(None) },
                    h3 { id: "dialog-title", if dialog_run { "Jalankan pembersihan sekarang?" } else { "Aktifkan pembersihan otomatis?" } }
                    div { id: "dialog-body",
                        ul {
                            if !dialog_boards.is_empty() { li { "Board: {dialog_boards}." } }
                            li { "Card Done tanpa aktivitas selama {a_days} hari diarsipkan." }
                            li { "Card di arsip lebih dari {p_days} hari dihapus permanen beserta komentar dan riwayatnya." }
                            li { "{dialog_summary}" }
                            if dialog_has_untracked {
                                li { "Backup database dibuat otomatis, lalu pencatatan khusus Pengudeck dipasang di board tersebut. Card yang sudah ada mulai dihitung umurnya dari hari ini." }
                            } else {
                                li { "Backup database dibuat otomatis sebelum card diubah." }
                            }
                        }
                    }
                    div { class: "settings-actions",
                        button { class: "btn", id: "dialog-cancel", onmounted: move |event| { spawn(async move { let _ = event.set_focus(true).await; }); }, onclick: move |_| confirm.set(None), "Batal" }
                        button { class: "btn", id: "dialog-confirm",
                            onclick: {
                                let dialog = dialog.clone();
                                move |_| {
                                    let dialog = dialog.clone();
                                    let policy = Some(dialog.policy.clone());
                                    confirm.set(None); busy.set(true); message.set(String::new());
                                    spawn(async move {
                                        if dialog.run_now {
                                            match request("run", json!({"fingerprint":dialog.fingerprint,"confirmed":true})).await {
                                                Ok(value) => {
                                                    let failed = value["errors"].as_array().is_some_and(|a| !a.is_empty());
                                                    last_run.set(value);
                                                    message.set(if failed { "Selesai dengan error. Periksa ringkasan di Lanjutan." } else { "Selesai. Muat ulang board untuk melihat hasilnya." }.into());
                                                }
                                                Err(error) => message.set(error.into()),
                                            }
                                        } else if let Some(policy) = policy {
                                            match request("save", json!({"policy":policy,"confirmed":true})).await {
                                                Ok(value) => { saved.set(value["policy"].clone()); message.set("Tersimpan. Pembersihan otomatis aktif.".into()); }
                                                Err(error) => message.set(error.into()),
                                            }
                                        }
                                        refresh(summary, fingerprint).await;
                                        busy.set(false);
                                    });
                                }
                            },
                            if dialog_run { "Jalankan" } else { "Aktifkan" } }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boards(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn form_builds_a_sorted_policy_and_rejects_bad_input() {
        assert_eq!(
            policy_from_form(false, " 30 ", "180", &boards(&["reporty", "default"])).unwrap(),
            json!({"enabled":false,"archive_days":30,"purge_days":180,"boards":["default","reporty"]})
        );
        for (a, p, b) in [
            ("0", "180", &["default"][..]),
            ("30", "3651", &["default"]),
            ("x", "1", &["default"]),
            ("30", "180", &[]),
        ] {
            assert!(
                policy_from_form(true, a, p, &boards(b)).is_none(),
                "{a} {p} {b:?}"
            );
        }
    }

    #[test]
    fn board_order_does_not_count_as_a_change() {
        let a = json!({"enabled":true,"archive_days":30,"purge_days":180,"boards":["b","a"]});
        let b = json!({"enabled":true,"archive_days":30,"purge_days":180,"boards":["a","b"]});
        assert_eq!(normalized(&a), normalized(&b));
    }

    #[test]
    fn error_codes_become_plain_messages_and_unknown_codes_stay_generic() {
        assert!(error_message("backup_failed").contains("tidak ada card yang diubah"));
        assert_eq!(error_message("anything /home/gnu SECRET"), FAILED);
        assert_eq!(error_message(""), FAILED);
    }

    #[test]
    fn summary_is_plain_language() {
        assert_eq!(
            summary_text(&Value::Null),
            "Jumlah card belum bisa dihitung."
        );
        assert!(
            summary_text(&json!({"archive":0,"purge":0,"untracked":["a","b"]})).contains("a, b")
        );
        assert_eq!(
            summary_text(&json!({"archive":0,"purge":0,"untracked":[]})),
            "Saat ini tidak ada card yang perlu dibersihkan."
        );
        assert_eq!(
            summary_text(&json!({"archive":2,"purge":1,"untracked":[]})),
            "Saat ini 2 card akan diarsipkan dan 1 card akan dihapus permanen."
        );
    }

    #[test]
    fn last_run_reports_counts_and_errors() {
        assert_eq!(last_run_text(&Value::Null), "Belum pernah berjalan.");
        let text = last_run_text(&json!({"at":0,"archived":["a"],"purged":[],"errors":["x"]}));
        assert!(text.contains("1 diarsipkan, 0 dihapus permanen"), "{text}");
        assert!(text.contains("Ada error"), "{text}");
    }

    #[test]
    fn panel_mounts_loading_with_no_write_handlers() {
        let mut dom =
            VirtualDom::new(|| rsx! { CleanupPanel { boards: vec!["default".to_string()] } });
        let rendered = format!("{:?}", dom.rebuild_to_vec());
        assert!(rendered.contains("LoadTemplate"), "{rendered}");
        assert_eq!(
            rendered.matches("NewEventListener").count(),
            0,
            "{rendered}"
        );
    }

    #[test]
    fn disabled_form_mounts_without_a_dialog() {
        let mut form = VirtualDom::new(|| {
            let initial = json!({"policy":{"enabled":false,"archive_days":30,"purge_days":180,"boards":["default"]},
                "summary":{"archive":0,"purge":0,"untracked":["default"]},"fingerprint":"","last_run":null});
            rsx! { CleanupForm { initial, boards: vec!["default".to_string(), "zaha".to_string()] } }
        });
        let rendered = format!("{:?}", form.rebuild_to_vec());
        assert!(rendered.contains("Bool(false)"), "{rendered}");
        assert!(rendered.contains("180"), "{rendered}");
        assert!(!rendered.contains("alertdialog"), "{rendered}");
    }
}
