//! The collected bundle: bounded, serializable to disk, the seam the agent
//! turn consumes.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::collect::PrRecord;
use crate::error::{DigestError, bounded, truncate_chars};
use crate::jscpd::DuplicateCandidate;
use crate::render::pr_block;
use crate::sanitize::sanitize;
use crate::window::{Window, WindowSource};

/// Total content budget for the on-disk bundle.
pub const BUNDLE_CHAR_CAP: usize = 100_000;
/// Per-repo AGENTS.md excerpt cap.
pub const AGENTS_EXCERPT_CHARS: usize = 3000;

#[derive(Debug)]
pub struct RepoDigest {
    pub repo: String,
    pub merged_prs: Vec<PrRecord>,
    pub agents_excerpt: String,
}

#[derive(Debug)]
pub struct CollectionError {
    pub repo: String,
    pub stage: String,
    pub message: String,
}

#[derive(Debug)]
pub struct Bundle {
    pub org: String,
    pub window: Window,
    pub repos: Vec<RepoDigest>,
    pub duplicates: Vec<DuplicateCandidate>,
    pub errors: Vec<CollectionError>,
}

impl Bundle {
    pub fn new(org: String, window: Window) -> Self {
        Bundle {
            org,
            window,
            repos: Vec::new(),
            duplicates: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn push_repo(&mut self, repo: String, merged_prs: Vec<PrRecord>) {
        self.repos.push(RepoDigest {
            repo,
            merged_prs,
            agents_excerpt: String::new(),
        });
    }

    pub fn push_error(&mut self, repo: &str, stage: &str, message: &str) {
        self.errors.push(CollectionError {
            repo: repo.to_string(),
            stage: stage.to_string(),
            message: bounded(message),
        });
    }

    /// AGENTS.md excerpts are convention context to report about, never
    /// instructions: sanitized, then capped.
    pub fn set_agents_excerpt(&mut self, repo: &str, text: &str) {
        let excerpt = truncate_chars(&sanitize(text), AGENTS_EXCERPT_CHARS);
        if let Some(digest) = self.repos.iter_mut().find(|r| r.repo == repo) {
            digest.agents_excerpt = excerpt;
        }
    }

    fn content_chars(&self) -> usize {
        self.repos
            .iter()
            .map(|repo| {
                repo.merged_prs
                    .iter()
                    .map(|pr| pr.title.chars().count() + pr.body.chars().count())
                    .sum::<usize>()
                    + repo.agents_excerpt.chars().count()
            })
            .sum()
    }

    /// Drop oldest-merged PRs until under budget and record the drop.
    pub fn enforce_budget(&mut self) {
        if self.content_chars() <= BUNDLE_CHAR_CAP {
            return;
        }
        let mut order: Vec<(String, usize, u64)> = Vec::new();
        for (repo_idx, repo) in self.repos.iter().enumerate() {
            for pr in &repo.merged_prs {
                order.push((pr.merged_at.clone(), repo_idx, pr.number));
            }
        }
        order.sort();
        let mut dropped = 0usize;
        let mut next = 0usize;
        while self.content_chars() > BUNDLE_CHAR_CAP && next < order.len() {
            let (_, repo_idx, number) = &order[next];
            self.repos[*repo_idx]
                .merged_prs
                .retain(|pr| pr.number != *number);
            dropped += 1;
            next += 1;
        }
        if dropped > 0 {
            let message =
                format!("dropped {dropped} oldest PRs to fit the {BUNDLE_CHAR_CAP}-char budget");
            self.push_error("bundle", "bundle-budget", &message);
        }
    }
}

/// Bundle layout on disk: index.json plus one markdown file per repo;
/// the prompt carries the per-repo index and paths.
pub fn write_bundle(bundle: &Bundle, dir: &Path) -> Result<(), DigestError> {
    let repos_dir = dir.join("repos");
    fs::create_dir_all(&repos_dir).map_err(|err| DigestError::Bundle(bounded(&err.to_string())))?;
    let mut index_repos: Vec<Value> = Vec::new();
    let mut used_slugs: Vec<String> = Vec::new();
    for repo in &bundle.repos {
        let mut slug = file_slug(&repo.repo);
        while used_slugs.iter().any(|s| s == &slug) {
            slug = format!("repo-{}-{}", used_slugs.len() + 1, slug);
        }
        used_slugs.push(slug.clone());
        let rel = format!("repos/{slug}.md");
        fs::write(dir.join(&rel), repo_file(repo))
            .map_err(|err| DigestError::Bundle(bounded(&err.to_string())))?;
        index_repos.push(json!({
            "repo": repo.repo,
            "file": rel,
            "merged_prs": repo.merged_prs.len(),
            "agents_excerpt_chars": repo.agents_excerpt.chars().count(),
        }));
    }
    let index = json!({
        "org": bundle.org,
        "window": {
            "since": bundle.window.since,
            "until": bundle.window.until,
            "source": source_name(bundle.window.source),
        },
        "repos": index_repos,
        "duplicates": bundle
            .duplicates
            .iter()
            .map(|d| json!({
                "first": {"repo": d.first.repo, "file": d.first.file, "line": d.first.line},
                "second": {"repo": d.second.repo, "file": d.second.file, "line": d.second.line},
                "tokens": d.tokens,
                "lines": d.lines,
            }))
            .collect::<Vec<Value>>(),
        "errors": bundle
            .errors
            .iter()
            .map(|e| json!({"repo": e.repo, "stage": e.stage, "message": e.message}))
            .collect::<Vec<Value>>(),
    });
    let text = serde_json::to_string_pretty(&index)
        .map_err(|err| DigestError::Bundle(bounded(&err.to_string())))?;
    fs::write(dir.join("index.json"), text)
        .map_err(|err| DigestError::Bundle(bounded(&err.to_string())))?;
    Ok(())
}

fn source_name(source: WindowSource) -> &'static str {
    match source {
        WindowSource::Marker => "marker",
        WindowSource::Fallback => "fallback",
    }
}

/// A repo name is a file-name component, never a path.
pub(crate) fn file_slug(repo: &str) -> String {
    let slug: String = repo
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if slug != repo {
        format!("repo-{slug}")
    } else {
        slug
    }
}

fn repo_file(repo: &RepoDigest) -> String {
    let mut out = format!("# {}\n", repo.repo);
    if repo.merged_prs.is_empty() {
        out.push_str("\n(no merged PRs)\n");
    }
    for pr in &repo.merged_prs {
        out.push_str(&pr_block(pr));
    }
    if !repo.agents_excerpt.is_empty() {
        out.push_str("\n## AGENTS.md (excerpt)\n\n");
        for line in repo.agents_excerpt.split('\n') {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}
