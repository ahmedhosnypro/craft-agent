// dups corpus: normalized window matching and deterministic reporting.
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

// Same tokens as BLOCK with hostile spacing and blank/comment noise.
const BLOCK_NOISY: &str = "\
# module header

fn  shared(alpha: String)   ->   u64   {

    let  beta =  alpha.len();
    // a dropped comment
    let gamma = beta + 1;
    let delta = gamma * 2;
    let epsilon = delta - 1;
    epsilon as u64
}

# tail comment
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

#[test]
fn shared_block_across_roots_is_one_candidate() {
    let root_a = temp_tree("scan-a");
    let root_b = temp_tree("scan-b");
    put(
        &root_a,
        "src/main.rs",
        &format!("use std::fmt;\n\n{BLOCK}\nfn tail() {{}}\n"),
    );
    put(&root_b, "lib/other.py", BLOCK_NOISY);
    let report = scan_dups(&roots_query(
        &[root_a.clone(), root_b.clone()],
        6,
        20,
        false,
    ))
    .unwrap();
    assert!(report.skipped.is_empty());
    assert_eq!(report.candidates.len(), 1);
    let candidate = &report.candidates[0];
    assert_eq!(candidate.lines, 7);
    assert_eq!(candidate.occurrences.len(), 2);
    assert_eq!(candidate.occurrences[0].root, root_a.to_str().unwrap());
    assert_eq!(candidate.occurrences[0].file, "src/main.rs");
    assert_eq!(candidate.occurrences[0].line, 3);
    assert_eq!(candidate.occurrences[1].root, root_b.to_str().unwrap());
    assert_eq!(candidate.occurrences[1].file, "lib/other.py");
    assert_eq!(candidate.occurrences[1].line, 3);
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn blocks_shorter_than_min_lines_do_not_qualify() {
    let short = "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\n";
    let root_a = temp_tree("min-a");
    let root_b = temp_tree("min-b");
    put(&root_a, "one.rs", short);
    put(&root_b, "two.rs", short);
    let report = scan_dups(&roots_query(
        &[root_a.clone(), root_b.clone()],
        6,
        20,
        false,
    ))
    .unwrap();
    assert!(report.candidates.is_empty());
    let report = scan_dups(&roots_query(
        &[root_a.clone(), root_b.clone()],
        4,
        20,
        false,
    ))
    .unwrap();
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].lines, 4);
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn two_disjoint_copies_inside_one_file_qualify() {
    let root = temp_tree("samefile");
    let filler = "fn fill_one() {}\nfn fill_two() {}\nfn fill_three() {}\n";
    let text = format!("{BLOCK}{filler}{BLOCK}");
    put(&root, "twin.rs", &text);
    let report = scan_dups(&roots_query(std::slice::from_ref(&root), 6, 20, false)).unwrap();
    assert_eq!(report.candidates.len(), 1);
    let candidate = &report.candidates[0];
    assert_eq!(candidate.lines, 7);
    assert_eq!(candidate.occurrences.len(), 2);
    assert_eq!(candidate.occurrences[0].line, 1);
    assert_eq!(candidate.occurrences[1].line, 11);
    assert_eq!(candidate.occurrences[0].file, "twin.rs");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn overlapping_windows_merge_into_one_span() {
    let long_block = "\
fn deep(one: u8) -> u8 {
    let two = one + 1;
    let three = two + 1;
    let four = three + 1;
    let five = four + 1;
    let six = five + 1;
    let seven = six + 1;
    let eight = seven + 1;
    nine(eight)
}
";
    let root_a = temp_tree("merge-a");
    let root_b = temp_tree("merge-b");
    put(&root_a, "long.rs", long_block);
    put(&root_b, "long_copy.rs", long_block);
    let report = scan_dups(&roots_query(
        &[root_a.clone(), root_b.clone()],
        6,
        20,
        false,
    ))
    .unwrap();
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].lines, 10);
    assert_eq!(report.candidates[0].occurrences.len(), 2);
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn adjacent_identical_windows_cover_the_full_run() {
    let root_a = temp_tree("run-a");
    let root_b = temp_tree("run-b");
    let repeated = "let same = 1;\n".repeat(10);
    let head = "fn head() {}\nfn neck() {}\nfn torso() {}\n";
    put(&root_a, "one.rs", &repeated);
    put(&root_b, "two.rs", &format!("{head}{repeated}"));
    let report = scan_dups(&roots_query(
        &[root_a.clone(), root_b.clone()],
        6,
        20,
        false,
    ))
    .unwrap();
    assert_eq!(report.candidates.len(), 1);
    let candidate = &report.candidates[0];
    assert_eq!(candidate.lines, 10);
    assert_eq!(candidate.occurrences.len(), 2);
    assert_eq!(candidate.occurrences[0].file, "one.rs");
    assert_eq!(candidate.occurrences[0].line, 1);
    assert_eq!(candidate.occurrences[1].file, "two.rs");
    assert_eq!(candidate.occurrences[1].line, 4);
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

#[test]
fn distinct_trees_report_no_candidates() {
    let root_a = temp_tree("none-a");
    let root_b = temp_tree("none-b");
    put(&root_a, "alpha.rs", "fn alpha() {}\nfn beta() {}\n");
    put(&root_b, "zeta.rs", "fn zeta() {}\nfn omega() {}\n");
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
fn min_lines_out_of_range_is_a_typed_error() {
    let root = temp_tree("arg");
    put(&root, "x.rs", "fn x() {}\n");
    for min_lines in [0, 1, 201] {
        let err = scan_dups(&roots_query(
            std::slice::from_ref(&root),
            min_lines,
            20,
            false,
        ))
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("must be 2..=200"), "{min_lines}: {text}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

fn lines_block(prefix: &str, count: usize) -> String {
    let mut text = String::new();
    for index in 1..=count {
        text.push_str(&format!("let {prefix}{index} = {index};\n"));
    }
    text
}

#[test]
fn candidates_rank_by_repeated_lines_with_stable_ties() {
    let root_a = temp_tree("rank-a");
    let root_b = temp_tree("rank-b");
    let block_a = lines_block("alpha", 8);
    let block_b = lines_block("bravo", 6);
    let block_c = lines_block("charlie", 6);
    let block_d = lines_block("delta", 6);
    put(&root_a, "a1.rs", &block_a);
    put(&root_b, "a2.rs", &block_a);
    put(&root_a, "b1.rs", &block_b);
    put(&root_b, "b2.rs", &block_b);
    put(&root_a, "c1.rs", &block_c);
    put(&root_b, "c2.rs", &block_c);
    put(&root_a, "d1.rs", &block_d);
    put(&root_b, "d2.rs", &block_d);
    let roots = [root_a.clone(), root_b.clone()];
    let report = scan_dups(&roots_query(&roots, 6, 20, false)).unwrap();
    let ranked: Vec<(&str, usize)> = report
        .candidates
        .iter()
        .map(|candidate| {
            (
                candidate.occurrences[0].file.as_str(),
                candidate.occurrences[0].line,
            )
        })
        .collect();
    assert_eq!(
        ranked,
        vec![("a1.rs", 1), ("b1.rs", 1), ("c1.rs", 1), ("d1.rs", 1)]
    );
    let capped = scan_dups(&roots_query(&roots, 6, 2, false)).unwrap();
    assert_eq!(capped.candidates.len(), 2);
    assert_eq!(capped.candidates[0].occurrences[0].file, "a1.rs");
    assert_eq!(capped.candidates[0].lines, 8);
    assert_eq!(capped.candidates[1].occurrences[0].file, "b1.rs");
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}
