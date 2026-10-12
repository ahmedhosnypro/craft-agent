// dups corpus: --cross-only filtering, cap bounds, and truncation markers.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::PathBuf;

use common::{put, temp_tree};
use craft_helpers::dups::{DupsQuery, scan_dups};

const BLOCK: &str = "\
fn shared(alpha: String) -> u64 {
    let beta = alpha.len();
    let gamma = beta + 1;
    let delta = gamma * 2;
    let epsilon = delta - 1;
    epsilon as u64
}
";

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

fn lines_block(prefix: &str, count: usize) -> String {
    let mut text = String::new();
    for index in 1..=count {
        text.push_str(&format!("let {prefix}{index} = {index};\n"));
    }
    text
}
#[test]
fn max_candidates_out_of_range_is_a_named_bound_error() {
    let root = temp_tree("arg-cap");
    put(&root, "x.rs", "fn x() {}\n");
    for value in [0, 501, 1000] {
        let err =
            scan_dups(&roots_query(std::slice::from_ref(&root), 6, value, false)).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("must be 1..=500"), "{value}: {text}");
        assert!(text.contains("--max-candidates"), "{value}: {text}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cross_only_keeps_multi_root_groups_only() {
    let root_a = temp_tree("cross-a");
    let root_b = temp_tree("cross-b");
    let shared = lines_block("shared", 7);
    put(&root_a, "solo_one.rs", BLOCK);
    put(&root_a, "solo_two.rs", BLOCK);
    put(&root_a, "shared.rs", &shared);
    put(&root_b, "shared.rs", &shared);
    let roots = [root_a.clone(), root_b.clone()];
    let report = scan_dups(&roots_query(&roots, 6, 20, false)).unwrap();
    assert_eq!(report.candidates.len(), 2);
    let report = scan_dups(&roots_query(&roots, 6, 20, true)).unwrap();
    assert_eq!(report.candidates.len(), 1);
    let candidate = &report.candidates[0];
    assert_eq!(candidate.occurrences.len(), 2);
    for occurrence in &candidate.occurrences {
        assert_eq!(occurrence.file, "shared.rs");
    }
    // Two occurrences with one rel path necessarily sit in different roots.
    assert_ne!(candidate.occurrences[0].root, candidate.occurrences[1].root);
    assert!(report.truncated.is_empty());
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn cross_only_without_multi_root_groups_is_empty() {
    let root_a = temp_tree("cross-solo-a");
    let root_b = temp_tree("cross-solo-b");
    put(&root_a, "solo_one.rs", BLOCK);
    put(&root_a, "solo_two.rs", BLOCK);
    put(&root_b, "distinct.rs", "fn distinct() {}\nfn other() {}\n");
    let roots = [root_a.clone(), root_b.clone()];
    let report = scan_dups(&roots_query(&roots, 6, 20, true)).unwrap();
    assert!(report.candidates.is_empty());
    assert!(report.truncated.is_empty());
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn cross_only_is_orthogonal_to_max_candidates() {
    let root_a = temp_tree("cross-cap-a");
    let root_b = temp_tree("cross-cap-b");
    let wide = lines_block("wide", 8);
    let narrow = lines_block("narrow", 6);
    put(&root_a, "wide.rs", &wide);
    put(&root_b, "wide.rs", &wide);
    put(&root_a, "narrow.rs", &narrow);
    put(&root_b, "narrow.rs", &narrow);
    put(&root_a, "solo.rs", BLOCK);
    put(&root_a, "solo_copy.rs", BLOCK);
    let roots = [root_a.clone(), root_b.clone()];
    let report = scan_dups(&roots_query(&roots, 6, 1, true)).unwrap();
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].occurrences[0].file, "wide.rs");
    assert_eq!(report.truncated.len(), 1);
    assert_eq!(report.truncated[0].field, "candidates");
    assert_eq!(report.truncated[0].cap, 1);
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn capped_candidates_carry_a_truncation_marker() {
    let root_a = temp_tree("marker-a");
    let root_b = temp_tree("marker-b");
    for (name, block) in [("one", 8), ("two", 7), ("three", 6)] {
        let text = lines_block(name, block);
        put(&root_a, &format!("{name}.rs"), &text);
        put(&root_b, &format!("{name}.rs"), &text);
    }
    let roots = [root_a.clone(), root_b.clone()];
    let report = scan_dups(&roots_query(&roots, 6, 2, false)).unwrap();
    assert_eq!(report.candidates.len(), 2);
    assert_eq!(report.truncated.len(), 1);
    let marker = &report.truncated[0];
    assert_eq!(marker.field, "candidates");
    assert_eq!(marker.cap, 2);
    assert!(marker.hint.contains("--max-candidates"), "{}", marker.hint);
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}
