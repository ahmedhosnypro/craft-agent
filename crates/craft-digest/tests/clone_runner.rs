// Corpus fixtures: unwrap() is the loud-test idiom here.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use craft_digest::bundle::Bundle;
use craft_digest::clone_runner::{CloneRunner, clone_all};
use craft_digest::window::{Window, WindowSource};

fn window() -> Window {
    Window {
        since: "2026-10-01".to_string(),
        until: "2026-10-11".to_string(),
        source: WindowSource::Marker,
    }
}

#[test]
fn clone_deadline_records_error_row() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/craft-digest-corpus")
        .join(format!("clone-deadline-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let git = dir.join("git");
    fs::write(&git, "#!/bin/sh\nsleep 30\n").unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();

    let runner = CloneRunner::with_program(&git).with_deadline(Duration::from_millis(200));
    let mut bundle = Bundle::new("storytold".to_string(), window());
    let scratch = dir.join("scratch");
    clone_all(
        &runner,
        "storytold",
        &["alpha".to_string()],
        &mut bundle,
        &scratch,
    )
    .unwrap();
    assert_eq!(bundle.errors.len(), 1);
    assert_eq!(bundle.errors[0].repo, "alpha");
    assert_eq!(bundle.errors[0].stage, "clone");
    assert!(bundle.errors[0].message.contains("deadline"));
    let _ = fs::remove_dir_all(&dir);
}
