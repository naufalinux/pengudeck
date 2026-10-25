# Hermes-dash — spesifikasi dan kontrak v1

Owner spesifikasi: Zaha. Task: `t_ca0383ee` (T1). Lisensi: GPL-3.0-only.
Dokumen ini adalah kontrak implementasi T2/T3 dan verifikasi independen T4,
bukan klaim bahwa server atau UI sudah tersedia.

## 1. Tujuan, batas, dan keputusan

Dashboard web **read-only** untuk melihat Kanban Hermes per board. Server
hanya bind ke **127.0.0.1:8787** sebagai konstanta, bukan `0.0.0.0`, `::`,
atau interface publik. Hosting domain adalah tahap nanti, bukan pekerjaan ini.

Di luar scope: membuat/mengubah/menghapus task atau board, autentikasi,
deploy, reverse proxy, domain, serta visualisasi dependency task.
Tidak ada endpoint mutasi. Task tidak mempunyai field `parents` dalam
kontrak sumber yang dipakai aplikasi ini; jangan mengasumsikan field tersebut.

Backend Rust: `hermes-dash-core` di `crates/core`, `hermes-dash-api` di
`crates/api`. API memakai axum. Core hanya boleh menambahkan `serde` dan
`serde_json`; API hanya axum, tokio, serde, serde_json, dan core. Dependency
lain memerlukan persetujuan. Tidak memasang paket sistem atau `cargo install`.

## 2. Sumber data dan hasil verifikasi CLI

### Daftar task

Sumber wajib:

```sh
hermes kanban --board <slug> list --json
```

JSON sumber adalah **array objek task**, bukan objek pembungkus. Field sumber
meliputi `id`, `title`, `status`, `assignee`, `created_by`, `created_at`,
`completed_at`, `result`, dan field tambahan. **Tidak ada field `parents`**.
Core v1 hanya memodelkan `id: String`, `title: String`, `status: Status`,
`assignee: Option<String>`; field tambahan diabaikan. `assignee` null atau
hilang menjadi `None`; tipe salah pada field yang dimodelkan adalah error.

`Status` memakai nilai JSON lowercase: `triage`, `todo`, `scheduled`, `ready`,
`running`, `blocked`, `review`, `done`, `archived`. String status baru dipetakan
ke `Unknown` melalui `#[serde(other)]`, lalu disembunyikan dari kolom.
`id`, `title`, dan `status` wajib ada. JSON rusak, non-array, atau elemen task
invalid harus menghasilkan `SourceError`, bukan data kosong yang tampak sukses.

Pada sesi T1 perintah read-only di atas untuk board `zaha` dicoba, tetapi
runtime menolak dengan exit 1 dan pesan berikut:

```text
kanban: delegate_task child contexts cannot mutate Kanban tasks or boards
```

Karena itu bentuk task di atas adalah kontrak dari brief, **bukan hasil
verifikasi live yang sukses di sesi ini**. Jangan menonaktifkan guard runtime.
Kevin harus menjalankan pembandingan live dari konteks yang mengizinkan
perintah read-only; bila tetap ditolak, verdict live adalah BLOCKED.

### Daftar board — format sudah diperiksa

Perintah berikut berhasil dieksekusi pada T1:

```sh
hermes kanban boards list
```

Output nyata berbentuk **tabel teks, bukan JSON**:

```text
    SLUG                      NAME                          COUNTS
●   default                   Default                       blocked=3, done=4, running=2, todo=4
    reporty                   Reporty                       blocked=3, done=4, running=2, todo=4
    zaha                      Zaha Team                     blocked=3, done=4, running=2, todo=4

Current board: default
Switch boards with `hermes kanban boards switch <slug>`.
```

Slug yang teramati berurutan: `default`, `reporty`, `zaha`. Angka COUNTS adalah
salinan observasi, bukan fixture jumlah task dan bukan sumber kebenaran API.
Parser mengambil token slug pertama pada setiap baris data setelah mengabaikan
marker opsional `●`; abaikan header, baris kosong, dan dua footer. Jangan
mengambil NAME atau COUNTS sebagai slug, jangan membelah nama berdasarkan
jumlah spasi tetap. Pertahankan urutan sumber, hilangkan duplikat. Tabel valid
tanpa baris data menghasilkan array kosong; output yang tidak dikenali menjadi
source error. Uji parser dengan dan tanpa marker, serta output malformed.

### Adapter sumber

`parse_list(&str) -> Result<Vec<Task>, SourceError>` dan trait `TaskSource`
dengan metode `list(&self, board: &str) -> Result<Vec<Task>, SourceError>`.
`CliSource { hermes_bin }` mengeksekusi program secara langsung dengan argv
`["kanban", "--board", board, "list", "--json"]`, **tanpa shell**.
Adapter daftar board memakai argv `["kanban", "boards", "list"]`; buat sumber
board yang dapat diinjeksi terpisah agar test router tidak menyentuh CLI live.
Jangan memperluas model Task dengan metadata board atau dependency.

stdout adalah data; exit nonzero/spawn gagal/parse gagal adalah error.
stderr mentah, path lokal, command line internal, dan data sensitif tidak boleh
dimasukkan ke error yang diterima klien. Jangan menjalankan CLI blocking di
executor async tanpa isolasi blocking. Dashboard tidak boleh mengganti current
board atau mengeksekusi CLI mutasi.

## 3. Kontrak HTTP

Semua respons API berupa JSON dengan `Content-Type: application/json`.
Nama field dan status lowercase; label tampilan tidak mengubah nilai wire.

### `GET /api/health`

HTTP 200, persis:

```json
{"ok":true}
```

Liveness saja, tidak memanggil Hermes atau mensyaratkan ada board.

### `GET /api/boards`

HTTP 200 berupa **array string slug**, tanpa wrapper atau objek NAME/COUNTS.
Contoh untuk hasil observasi T1 (bukan daftar hardcoded):

```json
["default","reporty","zaha"]
```

Daftar selalu diperoleh dari adapter `hermes kanban boards list`, bukan dari
asumsi board aktif atau nama direktori. Gagal sumber: HTTP 502,
`{"error":"source_unavailable"}`; jangan mengirim stderr mentah.

### `GET /api/boards/{slug}/tasks`

Slug adalah satu segmen nonkosong dengan karakter ASCII `[A-Za-z0-9_-]+`.
Slug invalid: HTTP 400, `{"error":"invalid_board"}`. Slug valid tetapi tidak
ada dalam daftar board: HTTP 404, `{"error":"board_not_found"}`. Sumber board
atau task gagal: HTTP 502, `{"error":"source_unavailable"}`.

HTTP 200 berisi **tepat enam kolom**, termasuk kolom kosong, urutan tetap:

| Indeks | status wire | Label tampilan |
| --- | --- | --- |
| 0 | todo | Todo |
| 1 | ready | Ready |
| 2 | running | Running |
| 3 | blocked | Blocked |
| 4 | review | Review |
| 5 | done | Done |

Contoh board tanpa task:

```json
{"columns":[{"status":"todo","tasks":[]},{"status":"ready","tasks":[]},{"status":"running","tasks":[]},{"status":"blocked","tasks":[]},{"status":"review","tasks":[]},{"status":"done","tasks":[]}]}
```

Setiap task dalam `tasks` diekspor dengan tepat empat field model minimal:

```json
{"id":"t_example","title":"Contoh ilustratif","status":"todo","assignee":null}
```

Contoh di atas ilustratif, bukan task live. `triage`, `scheduled`, `archived`,
dan `Unknown` disembunyikan. Urutan task dalam satu kolom mempertahankan urutan
input CLI. `COLUMNS = [Todo, Ready, Running, Blocked, Review, Done]` dan
`group_by_column(&[Task])` adalah satu sumber aturan grouping. Tidak ada
penghitungan parent atau dependency tersirat.

Metode mutasi tidak disediakan: POST pada route GET di atas menghasilkan 405.
Route API yang tidak dikenal menghasilkan 404; format error bawaan router untuk
405/route tak dikenal tidak ditetapkan sebagai bagian kontrak JSON di atas.

## 4. Acceptance per task dan bukti wajib

Setiap pemilik mencatat commit, perintah, exit code, jumlah test, dan output
nyata di handoff. TDD: simpan bukti RED sebelum implementasi dan GREEN sesudahnya.
Tidak ada klaim lulus hanya karena build berhasil. Perintah berikut adalah
acceptance untuk tahap berikutnya, **belum dijalankan sebagai verifikasi T1**.

### T2 — Archi, `t_cc8c9332`: core

- Fixture `crates/core/tests/fixtures/list_sample.json` berisi tepat **5 task**:
  dua todo, satu running, satu done, satu status string yang tidak dikenal.
  Gunakan minimal satu assignee null dan satu string; tambahkan field sumber
  ekstra untuk membuktikan forward compatibility tanpa field `parents`.
- Minimal **8 test**: parse fixture; JSON invalid; non-array; field wajib
  hilang/tipe salah; assignee hilang/null; status unknown; grouping dengan
  keenam kolom/urutan stabil; input kosong serta semua status tersembunyi.
  Unknown task terbuang sehingga fixture menghasilkan tepat **4 task terlihat**.
- Uji adapter tambahan dengan executable fake: argv persis, exit nonzero,
  dan sentinel stderr tidak muncul pada error publik. Tanpa shell.
- Tidak mengubah crate API atau menambah dependency di luar izin.

```sh
cargo test -p hermes-dash-core
```

Exit harus 0 dan tidak ada test gagal. Catat jumlah test aktual, jangan sekadar
menyalin minimum spesifikasi. Commit per langkah TDD.

### T3 — Archi, `t_ae8c3d44`: API

- Minimal **10 test router in-memory** dengan FakeSource: health; board list;
  board kosong; grouping fixture; board tak dikenal; slug invalid; task-source
  gagal; board-source gagal; POST ditolak; route tidak dikenal.
- Selain itu minimal **3 test parser board**: output observasi dengan marker,
  tanpa marker, dan malformed. Total minimum tahap ini **13 test**.
- Assert status HTTP dan payload, keenam kolom berurutan, null assignee,
  sanitasi sentinel stderr, serta jumlah fixture 2/0/1/0/0/1.
- Fake board source dipakai bersama fake task source; test router tidak bind
  socket dan tidak membutuhkan CLI live. Pakai kemampuan dependency yang
  diizinkan; jangan diam-diam menambahkan `tower` sebagai dev-dependency.
- Bind produksi adalah konstanta `127.0.0.1:8787`, tidak bisa dioverride menjadi
  wildcard. Tidak ada implementasi frontend sebelum keputusan user.

```sh
cargo test --workspace
cargo build -p hermes-dash-api
```

Keduanya exit 0. Catat jumlah test aktual dan bukti RED/GREEN.

### T4 — Kevin, `t_5e7f705e`: QA independen, tanpa ubah kode

1. Jalankan `cargo test --workspace` dan `cargo build -p hermes-dash-api`;
   keduanya harus exit 0. Hitung test aktual serta periksa coverage acceptance
   T2/T3, bukan hanya exit code.
2. Pastikan port belum dipakai. Jalankan `target/debug/hermes-dash-api` hanya
   untuk uji dan simpan PID milik proses sendiri. Jangan membunuh proses lain.
3. Jalankan empat request berikut (gunakan `curl --fail-with-body` agar HTTP
   error tidak tampak sukses):

   ```sh
   curl --fail-with-body -sS http://127.0.0.1:8787/api/health
   curl --fail-with-body -sS http://127.0.0.1:8787/api/boards
   curl --fail-with-body -sS http://127.0.0.1:8787/api/boards/default/tasks
   curl --fail-with-body -sS http://127.0.0.1:8787/api/boards/zaha/tasks
   ```

   Health tepat `{"ok":true}`; board list sesuai CLI, bukan hardcoded.
4. Jalankan sumber pembanding read-only:

   ```sh
   hermes kanban boards list
   hermes kanban --board default list --json
   hermes kanban --board zaha list --json
   ss -ltn 'sport = :8787'
   ```

   Bandingkan **6 jumlah per status per board** (12 perbandingan untuk kedua
   board) dan ID task, tanpa memasukkan status tersembunyi. Karena data live
   bisa berubah, ambil snapshot CLI sebelum dan sesudah respons API; ulangi
   jika snapshot berbeda. Ketidakstabilan bukan alasan memalsukan angka cocok.
   Socket harus hanya `127.0.0.1:8787`; `0.0.0.0:8787`, `*:8787`, atau
   `[::]:8787` adalah FAIL. Port dipakai proses lain: BLOCKED, bukan PASS.
5. Uji POST `/api/health` -> 405 dan satu slug valid yang dipastikan tidak ada
   -> 404. Periksa test sumber gagal membuktikan sanitasi stderr.
6. Hentikan PID server uji sendiri dengan `kill "$SERVER_PID"` dan konfirmasi
   socket uji sudah hilang dengan `ss -ltn 'sport = :8787'`.

Komentar QA memuat perintah, output, exit code, dan verdict **PASS / FAIL /
BLOCKED**. Guard CLI atau tool yang tidak tersedia berarti BLOCKED untuk bukti
live terkait, bukan bukti sukses. Implementasi selesai tidak sama dengan
acceptance produk; hanya verdict Kevin memenuhi gate ini. Jika FAIL, Zaha
membuat task koreksi Archi dan re-review Kevin yang bergantung padanya.

## 5. Pertanyaan terbuka untuk user — frontend belum diputuskan

**Apakah frontend akan menggunakan Dioxus web (memerlukan dx CLI 0.7.10 dan
toolchain WASM), atau alternatif tanpa WASM (HTML/CSS/JavaScript biasa yang
mengonsumsi API yang sama)?**

Dioxus menambah prasyarat build; alternatif tanpa WASM menghindari prasyarat
tersebut. Ini pertanyaan keputusan user, bukan izin memasang dx atau keputusan
arsitektur frontend oleh worker. T2/T3/T4 backend tidak perlu menunggu pilihan
ini; pekerjaan UI dan acceptance UI menunggu jawaban serta scope yang disetujui.

## 6. Handoff dan batas perubahan T1

T1 hanya menambahkan `docs/SPEC.md`, commit `docs: spec for hermes-dash`, tanpa
push dan tanpa install paket. Jalur yang sudah ada: T1 -> T2 core -> T3 API ->
T4 QA. Jangan membuat lane review duplikat. Spesifikasi ini menyelesaikan
kontrak backend; tidak menyatakan dashboard sudah selesai atau QA sudah PASS.
