//! The committed digest prompt: template loading and bundle summary.

use std::fs;
use std::path::{Path, PathBuf};

use crate::bundle::{Bundle, file_slug};
use crate::error::{DigestError, bounded, truncate_chars};

/// The prompt template's committed location, relative to the workspace.
pub const PROMPT_RELATIVE_PATH: &str = "config/digest-prompt.md";
/// Prompt reads are bounded; a bigger file is rejected, not truncated.
pub const PROMPT_CAP_BYTES: usize = 256 * 1024;
/// The summary handed to the model alongside the template.
const SUMMARY_CHAR_CAP: usize = 60_000;

pub fn default_prompt_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(PROMPT_RELATIVE_PATH)
}

/// Read the prompt template; every failure names the path it failed on.
pub fn load_prompt(path: &Path) -> Result<String, DigestError> {
    let named = |message: String| DigestError::Prompt {
        path: path.display().to_string(),
        message: bounded(&message),
    };
    let meta = fs::metadata(path).map_err(|err| named(err.to_string()))?;
    let size = meta.len();
    if size > PROMPT_CAP_BYTES as u64 {
        return Err(named(format!(
            "file exceeds the {PROMPT_CAP_BYTES}-byte cap ({size} bytes)"
        )));
    }
    fs::read_to_string(path).map_err(|err| named(err.to_string()))
}

/// The full model prompt: the template rules plus a bounded summary of the
/// collected bundle, including each repo's shallow-clone path.
pub fn build_digest_prompt(template: &str, bundle: &Bundle, clones_root: &Path) -> String {
    let mut prompt = String::from(template.trim_end());
    prompt.push_str(&bundle_summary(bundle, clones_root));
    prompt
}

/// One bounded text block describing the window, every repo row with its
/// merged PRs and clone path, the duplicate candidates, and error rows.
pub fn bundle_summary(bundle: &Bundle, clones_root: &Path) -> String {
    let window = &bundle.window;
    let mut out = format!(
        "\n\n## Bundle summary\n\norg: {}\nwindow: {}..{}\n",
        bundle.org, window.since, window.until
    );
    for repo in &bundle.repos {
        out.push_str(&format!(
            "\n### repo {} ({} merged PRs)\n",
            repo.repo,
            repo.merged_prs.len()
        ));
        out.push_str(&format!(
            "clone: {}\n",
            clones_root.join(file_slug(&repo.repo)).display()
        ));
        for pr in &repo.merged_prs {
            out.push_str(&format!(
                "\n- #{} {} (+{}/-{}) by {}: {}",
                pr.number, pr.title, pr.additions, pr.deletions, pr.author, pr.url
            ));
            for line in pr.body.lines().take(40) {
                out.push_str(&format!("\n  > {line}"));
            }
            out.push('\n');
        }
    }
    push_duplicates(&mut out, bundle);
    push_errors(&mut out, bundle);
    truncate_chars(&out, SUMMARY_CHAR_CAP)
}

fn push_duplicates(out: &mut String, bundle: &Bundle) {
    if bundle.duplicates.is_empty() {
        return;
    }
    out.push_str("\n### duplicate-code candidates\n");
    for dup in &bundle.duplicates {
        out.push_str(&format!(
            "\n- {} tokens: {}/{}:{} <-> {}/{}:{}",
            dup.tokens,
            dup.first.repo,
            dup.first.file,
            dup.first.line,
            dup.second.repo,
            dup.second.file,
            dup.second.line
        ));
    }
    out.push('\n');
}

fn push_errors(out: &mut String, bundle: &Bundle) {
    if bundle.errors.is_empty() {
        return;
    }
    out.push_str("\n### collection errors\n");
    for err in &bundle.errors {
        out.push_str(&format!(
            "\n- collection-error: {}: {}: {}",
            err.repo, err.stage, err.message
        ));
    }
    out.push('\n');
}
