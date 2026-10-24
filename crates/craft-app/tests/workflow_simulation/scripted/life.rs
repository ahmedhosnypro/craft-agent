//! The per-PR life script: the shard each pipeline prompt carries, the
//! stage children one PR's journey spawns, its filing lease, its
//! committer, and its own rollup section — plus the poll helpers that
//! keep every poll targeted at the ids its own spawn named.

use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;

use super::call;

/// The six synthetic PRs. Two are actionable — one rev-sync, one hoist —
/// and four are irrelevant, so the pipelines settle in mixed order and
/// exactly two PRs reach a lease and a committer while four end early.
pub(crate) const PRS: [&str; 6] = [
    "app-a#101|rev-sync|Cargo.lock",
    "app-b#201|hoist|crates/engine",
    "app-c#301|irrelevant|docs",
    "app-a#102|irrelevant|.github",
    "app-b#202|irrelevant|tests",
    "app-d#401|irrelevant|README.md",
];

/// The ids a spawn's acceptance line admitted.
pub(crate) fn accepted_ids(tool_result: &str) -> Vec<String> {
    tool_result
        .lines()
        .filter_map(|line| line.strip_prefix("accepted: "))
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

/// The ids a tool result names as still outstanding.
pub(crate) fn outstanding_ids(tool_result: &str) -> Vec<String> {
    let Some(line) = tool_result
        .lines()
        .find(|line| line.starts_with("outstanding:"))
    else {
        return Vec::new();
    };
    line.trim_start_matches("outstanding:")
        .split(',')
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

/// The next poll: by the ids still outstanding when the previous result
/// named them, otherwise a bounded wait. Each call is distinct, so the
/// stationarity guard never stops a working pool.
pub(crate) fn poll_call(ids: &[String], wait: u64) -> Vec<Result<ModelEvent, ProviderError>> {
    if ids.is_empty() {
        return call("poll", "poll_agents", &format!(r#"{{"waitSecs": {wait}}}"#));
    }
    let list = ids
        .iter()
        .map(|id| format!("\"{id}\""))
        .collect::<Vec<_>>()
        .join(", ");
    call(
        "poll-ids",
        "poll_agents",
        &format!(r#"{{"ids": [{list}], "waitSecs": {wait}}}"#),
    )
}

/// The PR shard a pipeline's prompt carries.
pub(crate) fn life_shard(prompt: &str) -> String {
    prompt
        .trim_start_matches("LIFE ")
        .split(" — ")
        .next()
        .unwrap_or("x")
        .trim()
        .to_string()
}

/// A shard whose class reaches a stage-2 child.
pub(crate) fn actionable(shard: &str) -> bool {
    matches!(
        shard.split('|').nth(1).unwrap_or("irrelevant"),
        "rev-sync" | "hoist" | "port"
    )
}

/// The stage-2 detached spawn for one PR: `rev-sync` goes to the
/// applier, `hoist` and `port` to the filer. Both return at admission.
pub(crate) fn emit_stage_two(shard: &str) -> Vec<Result<ModelEvent, ProviderError>> {
    let class = shard.split('|').nth(1).unwrap_or("irrelevant");
    let profile = if class == "rev-sync" {
        "sim-applier"
    } else {
        "sim-filer"
    };
    let number = shard.split('|').next().unwrap_or("x").trim();
    call(
        "stage2",
        "spawn_agent",
        &format!(
            r#"{{"agent":"{profile}","prompt":"STAGE2 {shard} — write your package for {number} locally","maxTurns":10000,"timeoutSecs":1800,"detached":true}}"#
        ),
    )
}

/// The pipeline's filing lease, taken before any committer runs: the
/// lease semantics the real `scripts/filing-lease.sh` enforces.
pub(crate) fn emit_lease(shard: &str) -> Vec<Result<ModelEvent, ProviderError>> {
    let class = shard.split('|').nth(1).unwrap_or("irrelevant");
    let (kind, slug) = if class == "rev-sync" {
        ("bump", shard.split('|').next().unwrap_or("x").trim())
    } else {
        ("finding", shard.split('|').next().unwrap_or("x").trim())
    };
    call(
        "lease",
        "bash",
        &format!(
            r#"{{"command":"mkdir -p run-ledger/filing && f=run-ledger/filing/{kind}.lease && n=$(grep -c . $f 2>/dev/null || true) && [ $n -ge 3 ] && exit 1; echo {slug} >> $f; echo leased run-ledger/filing/{kind}.lease"}}"#
        ),
    )
}

/// The pipeline's committer stage child: the run's only GitHub write,
/// idempotently, after the lease.
pub(crate) fn emit_committer(shard: &str) -> Vec<Result<ModelEvent, ProviderError>> {
    let class = shard.split('|').nth(1).unwrap_or("irrelevant");
    let slug = shard.split('|').next().unwrap_or("x").trim();
    if class == "rev-sync" {
        call(
            "bump-commit",
            "spawn_agent",
            &format!(
                r#"{{"agent":"sim-bump-committer","prompt":"COMMIT {slug} — record your bump outcome row locally","maxTurns":10000,"timeoutSecs":1800,"detached":true}}"#
            ),
        )
    } else {
        call(
            "file-commit",
            "spawn_agent",
            &format!(
                r#"{{"agent":"sim-filing-committer","prompt":"COMMIT {slug} — record your filing outcome row locally","maxTurns":10000,"timeoutSecs":1800,"detached":true}}"#
            ),
        )
    }
}

/// The pipeline's own rollup section: the PR's rollup, written by the
/// pipeline that owns its life.
pub(crate) fn emit_section(shard: &str) -> Vec<Result<ModelEvent, ProviderError>> {
    let number = shard.split('|').next().unwrap_or("x").trim();
    call(
        "section",
        "write_file",
        &format!(
            r#"{{"path":"run-ledger/rollup/{}.md","content":"{} section: class and evidence\n"}}"#,
            number, number
        ),
    )
}

/// The verdict line a classifier child returns, recovered from the path
/// its write landed at: the file's own first line is the verdict, so the
/// envelope's summary carries the PR number and its class.
pub(crate) fn verdict_line_for(tool_result: &str) -> String {
    for shard in &PRS {
        let number = shard.split('|').next().unwrap_or("x");
        let class = shard.split('|').nth(1).unwrap_or("irrelevant");
        if tool_result.contains(number) {
            return format!("{class} | {number} | scripted reason | files: scripted");
        }
    }
    String::from("unverified | unknown | scripted reason | files: none")
}
