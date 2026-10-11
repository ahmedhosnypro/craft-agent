// End-to-end digest pipeline corpus: fixture repos -> collect (fake gh) ->
// fake clones -> fake jscpd -> scripted provider turn -> bundle -> render
// -> scrub -> digest.md on disk; dry-run writes to the sink only.
// The std script lock is held across the pipeline await on purpose: it
// serializes whole tests on single-threaded test runtimes.
#![allow(clippy::unwrap_used)]
#![allow(clippy::await_holding_lock)]

use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::provider::{ModelEventStream, Provider, boxed};
use craft_core::types::{ModelRequest, StopReason, Usage};
use craft_digest::clone_runner::CloneRunner;
use craft_digest::collect::GhRunner;
use craft_digest::error::DigestError;
use craft_digest::jscpd_runner::JscpdRunner;
use craft_digest::pipeline::{
    DIGEST_OUTPUT_RELATIVE_PATH, DigestDeps, DigestOptions, DigestOutcome, run_digest,
};
use craft_digest::window::{WindowSource, date_from_epoch_secs, resolve_window_lookback};
use futures::stream;

fn temp_root(label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/craft-digest-corpus")
        .join(format!("pipeline-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_file(root: &Path, rel: &str, body: &str) -> PathBuf {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, body).unwrap();
    path
}

const EXCERPT: &str = "Shared error-handling conventions across the suite.";
const PROMPT_TEMPLATE: &str = "Digest rules: measured facts only, link every claim.";

/// Fake-runner scripts are exec'd right after being written; a parallel
/// fork in another test can inherit the still-open write fd and fail the
/// exec with ETXTBSY, so the script-driving tests serialize.
static SCRIPT_LOCK: Mutex<()> = Mutex::new(());

fn script_guard() -> MutexGuard<'static, ()> {
    SCRIPT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Fixture layout: `ws/` is the pipeline workspace, `bin/` holds the fake
/// runner scripts, `captures/` receives the argv each fake was called with.
struct Fixture {
    ws: PathBuf,
    bin: PathBuf,
    captures: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = temp_root(label);
        let fixture = Fixture {
            ws: root.join("ws"),
            bin: root.join("bin"),
            captures: root.join("captures"),
        };
        fs::create_dir_all(&fixture.bin).unwrap();
        fs::create_dir_all(&fixture.captures).unwrap();
        write_file(&fixture.ws, "config/digest-prompt.md", PROMPT_TEMPLATE);
        fixture
    }

    fn capture(&self, name: &str) -> PathBuf {
        self.captures.join(name)
    }
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn zero_usage() -> Usage {
    Usage {
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        incomplete: false,
    }
}

type ScriptedTurn = Vec<Result<ModelEvent, ProviderError>>;

fn text_turn(text: &str) -> ScriptedTurn {
    vec![
        Ok(ModelEvent::TextDelta {
            text: String::from(text),
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: StopReason::EndTurn,
            usage: zero_usage(),
        }),
    ]
}

/// Plays scripted turns in order, then repeats the last one forever.
struct ScriptedProvider {
    turns: Mutex<VecDeque<ScriptedTurn>>,
}

impl ScriptedProvider {
    fn new(turns: Vec<ScriptedTurn>) -> Arc<Self> {
        Arc::new(Self {
            turns: Mutex::new(turns.into()),
        })
    }
}

impl Provider for ScriptedProvider {
    async fn stream(&self, _request: ModelRequest) -> Result<ModelEventStream, ProviderError> {
        let mut turns = self
            .turns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let events = match turns.len() {
            0 => Vec::new(),
            1 => turns[0].clone(),
            _ => turns.pop_front().unwrap(),
        };
        Ok(boxed(stream::iter(events)))
    }
}

/// Fake gh: merged rows for `alpha` (one carrying the planted secret), a
/// hard failure for `beta`; both PRs merged today so the real-clock window
/// always contains them.
fn fake_gh(fixture: &Fixture, secret: &str) -> GhRunner {
    let body = format!(
        r#"today=$(date -u +%F)
case "$*" in
  *alpha*) cat <<JSON
[{{"number":11,"title":"Add pipeline","url":"https://example.invalid/pr/11","author":{{"login":"dev1"}},"mergedAt":"${{today}}T09:00:00Z","additions":40,"deletions":2,"labels":[{{"name":"feature"}}],"body":"leaked {secret} in the body"}},
 {{"number":12,"title":"Fix window","url":"https://example.invalid/pr/12","author":{{"login":"dev2"}},"mergedAt":"${{today}}T10:00:00Z","additions":8,"deletions":1,"labels":[],"body":"window math"}}]
JSON
  ;;
  *beta*) echo "rate limit exceeded (HTTP 403)" >&2; exit 1 ;;
esac"#
    );
    GhRunner::with_program(script(&fixture.bin, "gh", &body))
}

/// Fake git: `clone --depth 1 <url> <dir>` becomes a fixture tree with an
/// AGENTS.md; `beta` fails. The argv is captured for flag assertions.
fn fake_git(fixture: &Fixture, failing: &str) -> CloneRunner {
    let body = format!(
        r#"printf '%s\n' "$*" > {capture}
case "$4" in
  *{failing}*) echo "repository not found" >&2; exit 1 ;;
esac
repo=$(basename "$4" .git)
mkdir -p "$5/src"
printf '# %%s conventions\n\nFixture conventions text.\n' "$repo" > "$5/AGENTS.md"
printf 'fn shared() {{}}\n' > "$5/src/lib.rs""#,
        capture = fixture.capture("git-args").display(),
    );
    CloneRunner::with_program(script(&fixture.bin, "git", &body))
}

/// Fake jscpd: writes a v5-shaped json report into the `--output` dir and
/// captures its argv for flag assertions.
fn fake_jscpd(fixture: &Fixture) -> JscpdRunner {
    let body = format!(
        r#"printf '%s\n' "$*" > {capture}
out=""
prev=""
for a in "$@"; do
  if [ "$prev" = "--output" ]; then out="$a"; fi
  prev="$a"
done
mkdir -p "$out"
cat > "$out/jscpd-report.json" <<'JSON'
{{"duplicates":[{{"tokens":120,"lines":12,"firstFile":{{"name":"alpha/src/lib.rs","start":3}},"secondFile":{{"name":"beta/src/main.rs","start":7}}}}]}}
JSON"#,
        capture = fixture.capture("jscpd-args").display(),
    );
    JscpdRunner::with_program(script(&fixture.bin, "jscpd", &body))
}

fn deps(fixture: &Fixture, secret: &str, failing_clone: &str) -> DigestDeps {
    DigestDeps {
        gh: fake_gh(fixture, secret),
        clone_runner: fake_git(fixture, failing_clone),
        jscpd: fake_jscpd(fixture),
    }
}

fn options(fixture: &Fixture, dry_run: bool) -> DigestOptions {
    DigestOptions {
        org: String::from("storytold"),
        repos: vec![String::from("alpha"), String::from("beta")],
        lookback: Duration::from_secs(24 * 3600),
        dry_run,
        workspace_root: fixture.ws.clone(),
        prompt_path: None,
        max_turns: 40,
        max_total_tokens: 4_000_000,
        idle_timeout: None,
    }
}

async fn run(
    fixture: &Fixture,
    secret: &str,
    dry_run: bool,
    sink: &mut Vec<u8>,
) -> Result<DigestOutcome, DigestError> {
    let provider = ScriptedProvider::new(vec![text_turn(EXCERPT)]);
    run_digest(
        provider,
        &deps(fixture, secret, "beta"),
        &options(fixture, dry_run),
        sink,
    )
    .await
}

#[tokio::test]
async fn end_to_end_pipeline_renders_scrubbed_digest() {
    let _guard = script_guard();
    let fixture = Fixture::new("e2e");
    let secret = format!("ghp_{}", "a".repeat(36));
    let mut sink = Vec::new();
    let outcome = run(&fixture, &secret, false, &mut sink).await.unwrap();
    let digest_path = fixture.ws.join(DIGEST_OUTPUT_RELATIVE_PATH);
    assert_eq!(outcome.digest_path, Some(digest_path.clone()));
    let digest = read(&digest_path);
    assert!(digest.contains("# storytold digest "), "{digest}");
    assert!(digest.contains("## Merged PRs"));
    assert!(digest.contains("### alpha (2)"));
    assert!(digest.contains("### beta (0)"));
    assert!(digest.contains("[#11](https://example.invalid/pr/11) Add pipeline"));
    assert!(digest.contains("## Duplicate-code candidates"));
    assert!(digest.contains("[alpha/src/lib.rs:3]"), "{digest}");
    assert!(digest.contains("## Collection errors"));
    assert!(digest.contains("beta: gh: rate limit"), "{digest}");
    assert!(
        digest.contains("beta: clone: repository not found"),
        "{digest}"
    );
    assert!(!digest.contains(&secret), "secret survived scrub: {digest}");
    assert!(digest.contains("[scrubbed]"));
    assert!(digest.ends_with(" -->\n"), "{digest}");
    let last = digest.lines().last().unwrap();
    assert!(last.starts_with("<!-- digest-window: "), "{last}");

    let bundle = &outcome.bundle;
    assert_eq!(bundle.repos.len(), 2);
    assert!(
        bundle
            .repos
            .iter()
            .all(|repo| repo.agents_excerpt.contains(EXCERPT))
    );
    assert_eq!(bundle.duplicates.len(), 1);
    let stages: Vec<&str> = bundle.errors.iter().map(|err| err.stage.as_str()).collect();
    assert_eq!(stages, vec!["gh", "clone"]);
}

#[tokio::test]
async fn dry_run_writes_the_sink_and_no_files() {
    let _guard = script_guard();
    let fixture = Fixture::new("dry-run");
    let secret = format!("ghp_{}", "b".repeat(36));
    let mut sink = Vec::new();
    let outcome = run(&fixture, &secret, true, &mut sink).await.unwrap();
    assert!(outcome.digest_path.is_none());
    let text = String::from_utf8(sink).unwrap();
    assert!(text.contains("# storytold digest"), "{text}");
    assert!(text.contains("### alpha (2)"));
    assert!(text.contains("### beta (0)"));
    assert!(!text.contains(&secret), "secret survived scrub: {text}");
    assert!(!fixture.ws.join("out").exists());
    assert!(!fixture.ws.join(".craft-agent").exists());
}

#[test]
fn lookback_under_a_day_is_the_fallback_window() {
    let now = now_secs();
    let window = resolve_window_lookback(Duration::from_secs(23 * 3600)).unwrap();
    assert_eq!(window.until, date_from_epoch_secs(now));
    assert_eq!(window.since, date_from_epoch_secs(now - 86_400));
    assert!(matches!(window.source, WindowSource::Fallback));
}

#[test]
fn lookback_of_days_maps_to_since_dates() {
    let now = now_secs();
    let window = resolve_window_lookback(Duration::from_secs(3 * 86_400)).unwrap();
    assert_eq!(window.since, date_from_epoch_secs(now - 3 * 86_400));
    assert_eq!(window.until, date_from_epoch_secs(now));
    assert!(matches!(window.source, WindowSource::Marker));
}

#[test]
fn lookback_beyond_thirty_days_is_clamped() {
    let now = now_secs();
    let window = resolve_window_lookback(Duration::from_secs(366 * 86_400)).unwrap();
    assert_eq!(window.since, date_from_epoch_secs(now - 30 * 86_400));
    assert_eq!(window.until, date_from_epoch_secs(now));
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
