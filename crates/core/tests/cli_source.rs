#![cfg(unix)]

use hermes_dash_core::source::{CliSource, TaskSource};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct FakeCli { dir: PathBuf, bin: PathBuf }

impl FakeCli {
    fn new(body: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("hermes-dash-t_cc8c9332-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&dir).unwrap();
        let bin = dir.join("fake hermes");
        fs::write(&bin, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
        Self { dir, bin }
    }
    fn source(&self) -> CliSource { CliSource { hermes_bin: self.bin.clone() } }
}

impl Drop for FakeCli {
    fn drop(&mut self) { fs::remove_dir_all(&self.dir).unwrap(); }
}

#[test]
fn passes_exact_argv_and_treats_shell_metacharacters_as_literal_data() {
    let fake = FakeCli::new(r#"
[ "$#" -eq 5 ] || exit 11
[ "$1" = kanban ] || exit 12
[ "$2" = --board ] || exit 13
[ "$3" = 'board ; $(exit 99) * " spaced' ] || exit 14
[ "$4" = list ] || exit 15
[ "$5" = --json ] || exit 16
printf '%s' '[{"id":"one","title":"One","status":"todo"}]'
"#);
    let source: &dyn TaskSource = &fake.source();
    let tasks = source.list("board ; $(exit 99) * \" spaced").unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "one");
}

#[test]
fn nonzero_exit_rejects_valid_stdout_and_does_not_expose_stderr() {
    let fake = FakeCli::new("printf '[]'\nprintf 'STDERR_SECRET /private/path' >&2\nexit 7");
    let error = fake.source().list("default").unwrap_err();
    let public = format!("{error} {error:?}");
    assert!(!public.contains("STDERR_SECRET"));
    assert!(!public.contains("/private/path"));
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn spawn_failure_does_not_expose_executable_path() {
    let fake = FakeCli::new("exit 0");
    let source = CliSource { hermes_bin: fake.dir.join("SECRET_MISSING_BINARY") };
    let error = source.list("default").unwrap_err();
    assert!(!format!("{error} {error:?}").contains("SECRET_MISSING_BINARY"));
}

#[test]
fn successful_exit_with_malformed_stdout_is_an_error() {
    let fake = FakeCli::new("printf 'SECRET_INVALID_JSON'");
    let error = fake.source().list("default").unwrap_err();
    assert!(!format!("{error} {error:?}").contains("SECRET_INVALID_JSON"));
}

#[test]
fn successful_exit_with_non_utf8_stdout_is_an_error() {
    let fake = FakeCli::new("printf '\\377'");
    assert!(fake.source().list("default").is_err());
}

#[test]
fn stderr_is_not_parsed_as_data_on_success() {
    let fake = FakeCli::new("printf '[]'\nprintf 'STDERR_SECRET' >&2");
    assert!(fake.source().list("default").unwrap().is_empty());
}
