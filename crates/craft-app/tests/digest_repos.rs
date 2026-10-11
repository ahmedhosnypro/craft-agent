//! RED corpus: digest repo-set resolution (explicit list vs curated file).

use std::io::Write as _;
use std::path::PathBuf;

use craft_app::AppError;
use craft_app::digest_cmd::{load_curated_repos, resolve_repos};

fn fixture(name: &str, contents: &str) -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("craft-app-digest-{}-{name}", std::process::id()));
    let mut file = std::fs::File::create(&path)?;
    file.write_all(contents.as_bytes())?;
    Ok(path)
}

#[test]
fn curated_list_skips_comments_and_blanks() {
    let path = fixture(
        "curated.txt",
        "# header\n\nphotocraft\n  artcraft  \n# tail\nfilmcraft\n",
    )
    .expect("fixture builds");
    let repos = load_curated_repos(&path).expect("curated list parses");
    assert_eq!(repos, vec!["photocraft", "artcraft", "filmcraft"]);
}

#[test]
fn missing_curated_list_is_a_named_error() {
    let path = std::env::temp_dir().join("craft-app-digest-missing-nope.txt");
    let err = load_curated_repos(&path).expect_err("missing list must fail");
    assert!(matches!(err, AppError::CuratedRepos { .. }));
    assert!(err.to_string().contains("missing-nope"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn comment_only_curated_list_is_empty_error() {
    let path = fixture("empty.txt", "# nothing but comments\n\n").expect("fixture builds");
    let err = load_curated_repos(&path).expect_err("empty list must fail");
    assert!(matches!(err, AppError::EmptyRepoList { .. }));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn explicit_repos_win_over_the_curated_file() {
    let curated = fixture("ignored.txt", "photocraft\n").expect("fixture builds");
    let repos = resolve_repos(Some(vec!["a".to_string(), "b".to_string()]), &curated)
        .expect("explicit repos resolve");
    assert_eq!(repos, vec!["a", "b"]);
    let _ = std::fs::remove_file(&curated);
}

#[test]
fn explicit_repos_trim_and_drop_empties() {
    let curated = fixture("unused.txt", "photocraft\n").expect("fixture builds");
    let repos = resolve_repos(
        Some(vec!["  x  ".to_string(), "".to_string(), "y".to_string()]),
        &curated,
    )
    .expect("explicit repos resolve");
    assert_eq!(repos, vec!["x", "y"]);
    let _ = std::fs::remove_file(&curated);
}

#[test]
fn explicit_repos_all_empty_is_a_named_error() {
    let curated = fixture("unused2.txt", "photocraft\n").expect("fixture builds");
    let err = resolve_repos(Some(vec![" ".to_string(), "".to_string()]), &curated)
        .expect_err("blank repos must fail");
    assert!(matches!(err, AppError::EmptyReposFlag));
    let _ = std::fs::remove_file(&curated);
}
