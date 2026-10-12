// dups corpus: hostile trees — skips, caps, and deterministic output.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::PathBuf;

use common::{put, put_bytes, temp_tree};
use craft_helpers::dups::{
    DupsQuery, MAX_DEPTH, MAX_FILE_BYTES, MAX_FILES_PER_ROOT, MAX_WINDOWS, scan_dups,
};

fn roots_query(
    roots: &[PathBuf],
    min_lines: usize,
    max_candidates: usize,
    cross_only: bool,
) -> DupsQuery {
    DupsQuery {
        roots: roots.to_vec(),
        min_lines,
        max_candidates,
        cross_only,
    }
}

#[test]
fn invalid_utf8_files_get_skip_notes() {
    let root_a = temp_tree("host-a");
    let root_b = temp_tree("host-b");
    put(&root_a, "good.rs", "fn good() {}\n");
    put_bytes(&root_a, "blob.rs", &[0xff, 0xfe, 0x00, 0xfd, b'\n', 0xfc]);
    put(&root_b, "other.rs", "fn other() {}\n");
    let report = scan_dups(&roots_query(
        &[root_a.clone(), root_b.clone()],
        6,
        20,
        false,
    ))
    .unwrap();
    assert!(report.candidates.is_empty());
    assert_eq!(report.skipped.len(), 1);
    let note = &report.skipped[0];
    assert!(note.contains("blob.rs"), "{note}");
    assert!(note.contains("utf-8"), "{note}");
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn oversized_files_get_skip_notes() {
    let root = temp_tree("host-size");
    let huge = "x".repeat((MAX_FILE_BYTES + 1) as usize);
    put(&root, "huge.rs", &huge);
    put(&root, "tiny.rs", "fn tiny() {}\n");
    let report = scan_dups(&roots_query(std::slice::from_ref(&root), 6, 20, false)).unwrap();
    assert!(report.candidates.is_empty());
    let oversized: Vec<&String> = report
        .skipped
        .iter()
        .filter(|note| note.contains("huge.rs"))
        .collect();
    assert_eq!(oversized.len(), 1);
    assert!(oversized[0].contains("size"), "{}", oversized[0]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn too_deep_trees_get_skip_notes() {
    let root = temp_tree("host-deep");
    let mut deep = String::new();
    for level in 0..=MAX_DEPTH + 2 {
        deep.push_str(&format!("level{level}/"));
    }
    deep.push_str("bottom.rs");
    put(&root, &deep, "fn bottom() {}\n");
    put(&root, "top.rs", "fn top() {}\n");
    let report = scan_dups(&roots_query(std::slice::from_ref(&root), 6, 20, false)).unwrap();
    assert!(report.candidates.is_empty());
    assert_eq!(report.skipped.len(), 1);
    assert!(report.skipped[0].contains("depth"), "{}", report.skipped[0]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn vendored_directories_are_ignored_silently() {
    let root = temp_tree("host-vendor");
    let dup =
        "let one = 1;\nlet two = 2;\nlet three = 3;\nlet four = 4;\nlet five = 5;\nlet six = 6;\n";
    put(&root, "real.rs", dup);
    put(&root, "target/generated.rs", dup);
    put(&root, ".git/bundle.rs", dup);
    put(&root, "node_modules/pkg.rs", dup);
    let report = scan_dups(&roots_query(std::slice::from_ref(&root), 6, 20, false)).unwrap();
    assert!(report.candidates.is_empty());
    assert!(report.skipped.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn file_count_per_root_is_capped() {
    let root = temp_tree("host-files");
    for index in 0..=MAX_FILES_PER_ROOT {
        let rel = format!("f{index:04}.rs");
        let body = format!("let marker{index} = {index};\n");
        put(&root, &rel, &body);
    }
    let report = scan_dups(&roots_query(std::slice::from_ref(&root), 6, 20, false)).unwrap();
    let capped: Vec<&String> = report
        .skipped
        .iter()
        .filter(|note| note.contains("file cap"))
        .collect();
    assert_eq!(capped.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn non_source_files_do_not_consume_the_file_cap() {
    let root_a = temp_tree("cap-nosrc-a");
    let root_b = temp_tree("cap-nosrc-b");
    for index in 0..MAX_FILES_PER_ROOT {
        put(&root_a, &format!("blob{index:04}.bin"), "not source");
    }
    let dup =
        "let one = 1;\nlet two = 2;\nlet three = 3;\nlet four = 4;\nlet five = 5;\nlet six = 6;\n";
    put(&root_a, "real.rs", dup);
    put(&root_b, "twin.rs", dup);
    let report = scan_dups(&roots_query(
        &[root_a.clone(), root_b.clone()],
        6,
        20,
        false,
    ))
    .unwrap();
    assert!(report.skipped.is_empty());
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].lines, 6);
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn window_total_is_capped() {
    let root = temp_tree("host-windows");
    let lines_per_file = 1_000;
    let files = MAX_WINDOWS / (lines_per_file - 5) + 2;
    for file in 0..files {
        let mut body = String::new();
        for line in 0..lines_per_file {
            body.push_str(&format!("f{file}l{line}\n"));
        }
        let rel = format!("w{file:03}.rs");
        put(&root, &rel, &body);
    }
    let report = scan_dups(&roots_query(std::slice::from_ref(&root), 6, 20, false)).unwrap();
    let capped: Vec<&String> = report
        .skipped
        .iter()
        .filter(|note| note.contains("window cap"))
        .collect();
    assert_eq!(capped.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_missing_root_is_a_note_and_never_fatal() {
    let root = temp_tree("host-missing");
    let absent = temp_tree("host-absent");
    let _ = std::fs::remove_dir_all(&absent);
    put(&root, "one.rs", "fn one() {}\n");
    let report = scan_dups(&roots_query(&[absent.clone(), root.clone()], 6, 20, false)).unwrap();
    assert!(report.candidates.is_empty());
    assert_eq!(report.skipped.len(), 1);
    assert!(
        report.skipped[0].contains("not a directory"),
        "{}",
        report.skipped[0]
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn non_source_extensions_are_never_read() {
    let root_a = temp_tree("host-ext-a");
    let root_b = temp_tree("host-ext-b");
    let dup =
        "let one = 1;\nlet two = 2;\nlet three = 3;\nlet four = 4;\nlet five = 5;\nlet six = 6;\n";
    put(&root_a, "code.bin", dup);
    put(&root_b, "code.bin", dup);
    let report = scan_dups(&roots_query(
        &[root_a.clone(), root_b.clone()],
        6,
        20,
        false,
    ))
    .unwrap();
    assert!(report.candidates.is_empty());
    assert!(report.skipped.is_empty());
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn repeated_scans_produce_identical_output() {
    let root_a = temp_tree("host-det-a");
    let root_b = temp_tree("host-det-b");
    let dup =
        "let one = 1;\nlet two = 2;\nlet three = 3;\nlet four = 4;\nlet five = 5;\nlet six = 6;\n";
    put(&root_a, "one.rs", &format!("fn head() {{}}\n\n{dup}"));
    put(&root_b, "two.py", &format!("# c\n\nlet  one =  1;\n{dup}"));
    let roots = [root_a.clone(), root_b.clone()];
    let first =
        serde_json::to_string(&scan_dups(&roots_query(&roots, 6, 20, false)).unwrap()).unwrap();
    let second =
        serde_json::to_string(&scan_dups(&roots_query(&roots, 6, 20, false)).unwrap()).unwrap();
    assert_eq!(first, second);
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn window_cap_note_names_the_filling_root() {
    let root_a = temp_tree("win-root-a");
    let root_b = temp_tree("win-root-b");
    let lines_per_file = 1_000;
    let files = MAX_WINDOWS / (lines_per_file - 5) + 2;
    for file in 0..files {
        let mut body = String::new();
        for line in 0..lines_per_file {
            body.push_str(&format!("f{file}l{line}\n"));
        }
        put(&root_a, &format!("w{file:03}.rs"), &body);
    }
    put(&root_b, "later.rs", "fn later() {}\n");
    let report = scan_dups(&roots_query(
        &[root_a.clone(), root_b.clone()],
        6,
        20,
        false,
    ))
    .unwrap();
    let note = &report.skipped[0];
    assert!(note.contains("window cap"), "{note}");
    assert!(note.contains(root_a.to_str().unwrap()), "{note}");
    assert_eq!(report.truncated.len(), 1);
    let marker = &report.truncated[0];
    assert_eq!(marker.field, "windows");
    assert_eq!(marker.cap, MAX_WINDOWS);
    assert!(
        marker.hint.contains(root_a.to_str().unwrap()),
        "{}",
        marker.hint
    );
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn file_cap_note_names_the_root_and_marks_truncation() {
    let root_a = temp_tree("fcap-a");
    let root_b = temp_tree("fcap-b");
    for index in 0..=MAX_FILES_PER_ROOT {
        put(&root_a, &format!("g{index:04}.rs"), "fn g() {}\n");
    }
    put(&root_b, "other.rs", "fn other() {}\n");
    let report = scan_dups(&roots_query(
        &[root_a.clone(), root_b.clone()],
        6,
        20,
        false,
    ))
    .unwrap();
    let note = &report.skipped[0];
    assert!(note.contains("file cap"), "{note}");
    assert!(note.contains(root_a.to_str().unwrap()), "{note}");
    assert_eq!(report.truncated.len(), 1);
    let marker = &report.truncated[0];
    assert_eq!(marker.field, "files");
    assert_eq!(marker.cap, MAX_FILES_PER_ROOT);
    assert!(
        marker.hint.contains(root_a.to_str().unwrap()),
        "{}",
        marker.hint
    );
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}
