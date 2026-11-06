use dioxus::prelude::*;
use serde_json::{json, Value};

fn policy_from_form(enabled: bool, archive: &str, purge: &str, boards: &str) -> Option<Value> {
    let archive: u32 = archive.parse().ok()?;
    let purge: u32 = purge.parse().ok()?;
    let boards: Vec<_> = boards.split(',').map(str::trim).collect();
    if !(1..=3650).contains(&archive)
        || !(1..=3650).contains(&purge)
        || boards.is_empty()
        || boards.len() > 32
        || boards.iter().any(|s| {
            s.is_empty()
                || s.len() > 64
                || !s
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        })
        || boards
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != boards.len()
    {
        return None;
    }
    Some(json!({"enabled":enabled,"archive_days":archive,"purge_days":purge,"boards":boards}))
}

async fn request(operation: &str, payload: Value) -> Result<Value, &'static str> {
    let failed = "Permintaan retensi gagal. Muat ulang Settings dan preview sebelum mencoba lagi.";
    let eval = document::eval(include_str!("retention.js"));
    eval.send((operation.to_string(), payload))
        .map_err(|_| failed)?;
    let response: String = eval.join().await.map_err(|_| failed)?;
    let data: Value = serde_json::from_str(&response).map_err(|_| failed)?;
    if data["ok"] == true {
        Ok(data["data"].clone())
    } else {
        Err(failed)
    }
}

#[component]
pub fn RetentionPanel() -> Element {
    let mut data = use_resource(|| request("get", json!({})));
    rsx! {
        section { class: "settings-panel", "aria-label": "Settings retensi",
            h2 { "Retensi Kanban" }
            p { "Arsipkan kartu Done yang lama, lalu hapus permanen arsip yang melewati masa simpan. Kartu belum selesai tetap dipertahankan." }
            p { class: "notice", "Otomatisasi nonaktif secara default. Hanya berjalan saat API Pengudeck hidup; maksimal satu kali per 24 jam, 50 kartu per run. Aktivitas baru menunda retensi. Backup database dibuat sebelum perubahan, bukan backup file workspace." }
            match data.read().as_ref() {
                None => rsx! { p { role: "status", "Memuat Settings…" } },
                Some(Err(message)) => rsx! { p { role: "alert", "{message}" } button { class: "btn", onclick: move |_| data.restart(), "Coba lagi" } },
                Some(Ok(value)) => rsx! { RetentionForm { key: "{value}", initial: value.clone() } },
            }
            button { class: "btn", onclick: move |_| data.restart(), "Muat ulang Settings" }
        }
    }
}

#[component]
fn RetentionForm(initial: Value) -> Element {
    let original = initial["policy"].clone();
    let init_enabled = original["enabled"].as_bool().unwrap_or(false);
    let init_archive = original["archive_days"].to_string();
    let init_purge = original["purge_days"].to_string();
    let init_boards = original["boards"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let mut saved = use_signal(move || original);
    let mut enabled = use_signal(move || init_enabled);
    let mut archive = use_signal(move || init_archive);
    let mut purge = use_signal(move || init_purge);
    let mut boards = use_signal(move || init_boards);
    let mut enable_confirmation = use_signal(String::new);
    let mut apply_confirmation = use_signal(String::new);
    let mut track_confirmation = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let mut preview = use_signal(|| None::<Value>);
    let init_attempt = initial["last_attempt"].clone();
    let mut attempt = use_signal(move || init_attempt);
    let mut last = use_signal(move || initial["last_run"].clone());
    let policy = policy_from_form(enabled(), &archive(), &purge(), &boards());
    let dirty = policy.as_ref() != Some(&saved());
    let can_save = policy.is_some() && (!enabled() || enable_confirmation() == "ENABLE RETENTION");
    let fingerprint = preview
        .read()
        .as_ref()
        .and_then(|v| v["fingerprint"].as_str())
        .unwrap_or_default()
        .to_string();
    let tracked = preview
        .read()
        .as_ref()
        .is_some_and(|v| v["tracking_required"].as_array().is_some_and(Vec::is_empty));
    let can_apply = !dirty
        && tracked
        && !fingerprint.is_empty()
        && apply_confirmation() == "ARCHIVE AND DELETE";
    let attempt_text = attempt()
        .get("at")
        .and_then(Value::as_i64)
        .map(|at| {
            format!(
                "{} — {}",
                crate::format_last_updated(at),
                attempt()["status"].as_str().unwrap_or("unknown")
            )
        })
        .unwrap_or_else(|| "Belum ada percobaan.".into());
    let preview_text = preview
        .read()
        .as_ref()
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
        .unwrap_or_default();
    let last_text = if last().is_null() {
        "Belum ada eksekusi.".to_string()
    } else {
        serde_json::to_string_pretty(&last()).unwrap_or_default()
    };
    rsx! {
        fieldset { disabled: busy(),
            legend { "Kebijakan tersimpan di server, bukan browser" }
            p { class: "notice", "Sebelum retensi dapat berjalan, inisialisasi pencatatan pada board tersimpan. Ini menambah tabel dan trigger SQLite khusus Pengudeck setelah backup. Kartu lama memulai masa observasi baru; tidak langsung dihapus. Pencatatan tetap berjalan saat otomatisasi dimatikan agar re-archive dan aktivitas tidak hilang setelah GC." }
            label { r#for: "tracking-confirmation", "Ketik INITIALIZE TRACKING untuk mengizinkan pencatatan database" }
            input { id: "tracking-confirmation", value: track_confirmation(), autocomplete: "off", oninput: move |event| track_confirmation.set(event.value()) }
            button { class: "btn", disabled: dirty || track_confirmation() != "INITIALIZE TRACKING",
                onclick: move |_| {
                    busy.set(true); preview.set(None); message.set(String::new());
                    spawn(async move {
                        match request("track", json!({"confirmation":track_confirmation()})).await {
                            Ok(_) => message.set("Pencatatan retensi siap. Jalankan preview baru.".into()),
                            Err(error) => message.set(error.into()),
                        }
                        track_confirmation.set(String::new()); busy.set(false);
                    });
                }, "Inisialisasi pencatatan" }
            label { r#for: "retention-enabled", class: "check-label",
                input { id: "retention-enabled", r#type: "checkbox", checked: enabled(),
                    onchange: move |event| { enabled.set(event.checked()); preview.set(None); apply_confirmation.set(String::new()); } }
                "Jalankan otomatis setiap hari saat API hidup"
            }
            div { class: "retention-fields",
                label { r#for: "archive-days", "Arsipkan Done setelah (hari)" }
                input { id: "archive-days", r#type: "number", min: "1", max: "3650", value: archive(),
                    oninput: move |event| { archive.set(event.value()); preview.set(None); } }
                label { r#for: "purge-days", "Hapus setelah berada di arsip (hari)" }
                input { id: "purge-days", r#type: "number", min: "1", max: "3650", value: purge(),
                    oninput: move |event| { purge.set(event.value()); preview.set(None); } }
                label { r#for: "retention-boards", "Board (slug, pisahkan dengan koma)" }
                input { id: "retention-boards", value: boards(), placeholder: "default, reporty, zaha",
                    oninput: move |event| { boards.set(event.value()); preview.set(None); } }
            }
            if enabled() {
                label { r#for: "enable-confirmation", "Ketik ENABLE RETENTION untuk mengizinkan arsip dan hapus otomatis" }
                input { id: "enable-confirmation", value: enable_confirmation(), autocomplete: "off",
                    oninput: move |event| enable_confirmation.set(event.value()) }
            }
            if policy.is_none() { p { role: "alert", "Isi hari 1–3650 dan slug board yang valid tanpa duplikat." } }
            if dirty { p { "Ada perubahan. Simpan kebijakan sebelum preview." } }
            div { class: "settings-actions",
                button { class: "btn", disabled: !can_save,
                    onclick: move |_| {
                        let Some(policy) = policy.clone() else { return; };
                        busy.set(true); preview.set(None); message.set(String::new());
                        spawn(async move {
                            match request("save", json!({"policy":policy,"confirmation":enable_confirmation()})).await {
                                Ok(value) => { saved.set(value["policy"].clone()); attempt.set(value["last_attempt"].clone()); enable_confirmation.set(String::new()); message.set("Kebijakan tersimpan.".into()); }
                                Err(error) => message.set(error.into()),
                            }
                            busy.set(false);
                        });
                    }, "Simpan kebijakan" }
                button { class: "btn", disabled: dirty,
                    onclick: move |_| {
                        busy.set(true); preview.set(None); apply_confirmation.set(String::new()); message.set(String::new());
                        spawn(async move {
                            match request("preview", json!({})).await {
                                Ok(value) => { preview.set(Some(value)); message.set("Preview saja; belum ada kartu yang diubah.".into()); }
                                Err(error) => message.set(error.into()),
                            }
                            busy.set(false);
                        });
                    }, "Preview / dry-run" }
            }
            if preview.read().is_some() {
                h3 { "Kandidat berdasarkan kebijakan tersimpan" }
                pre { class: "retention-output", "{preview_text}" }
                p { class: "notice", "Hapus permanen menghilangkan kartu beserta komentar dan riwayatnya dari board. Backup database tetap tersimpan. Preview yang berubah harus diulang." }
                label { r#for: "apply-confirmation", "Ketik ARCHIVE AND DELETE untuk menjalankan preview ini" }
                input { id: "apply-confirmation", value: apply_confirmation(), autocomplete: "off",
                    oninput: move |event| apply_confirmation.set(event.value()) }
                button { class: "btn", disabled: !can_apply,
                    onclick: move |_| {
                        let fingerprint = fingerprint.clone();
                        busy.set(true); message.set(String::new());
                        spawn(async move {
                            match request("apply", json!({"fingerprint":fingerprint,"confirmation":apply_confirmation()})).await {
                                Ok(value) => {
                                    let failed = value["errors"].as_array().is_some_and(|a| !a.is_empty());
                                    attempt.set(json!({"at":value["at"],"status":if failed { "failed" } else { "finished" }}));
                                    last.set(value);
                                    message.set(if failed { "Eksekusi berhenti dengan error; periksa hasil dan buat preview baru." } else { "Eksekusi selesai; periksa hasil lalu muat ulang board." }.into());
                                }
                                Err(error) => message.set(error.into()),
                            }
                            preview.set(None); apply_confirmation.set(String::new()); busy.set(false);
                        });
                    }, "Jalankan arsip / hapus" }
            }
        }
        p { role: "status", "aria-live": "polite", if busy() { "Memproses…" } else { "{message()}" } }
        h3 { "Hasil eksekusi terakhir" }
        p { "Percobaan terakhir: {attempt_text}" }
        pre { class: "retention-output", "{last_text}" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_form_validates_days_and_requires_explicit_boards() {
        assert!(policy_from_form(false, "0", "180", "default").is_none());
        assert!(policy_from_form(false, "30", "180", "../bad").is_none());
        assert!(policy_from_form(false, "30", "180", "").is_none());
        assert_eq!(
            policy_from_form(false, "30", "180", "default, reporty").unwrap(),
            json!({"enabled":false,"archive_days":30,"purge_days":180,"boards":["default","reporty"]})
        );
    }
    #[test]
    fn panel_mounts_loading_without_enabling_automatic_deletion() {
        let mut dom = VirtualDom::new(RetentionPanel);
        let rendered = format!("{:?}", dom.rebuild_to_vec());
        assert!(rendered.contains("LoadTemplate"), "{rendered}");
        // Static copy lives in templates; no write event handler is mounted while loading.
        assert_eq!(rendered.matches("NewEventListener").count(), 1); // read-only reload
        let mut form = VirtualDom::new(
            || rsx! { RetentionForm { initial: json!({"policy":{"enabled":false,"archive_days":30,"purge_days":180,"boards":["default"]},"last_run":null}) } },
        );
        let controls = format!("{:?}", form.rebuild_to_vec());
        assert!(controls.contains("Bool(false)"));
        assert!(controls.contains("30"));
        assert!(controls.contains("180"));
        assert!(controls.contains("default"));
    }
}
