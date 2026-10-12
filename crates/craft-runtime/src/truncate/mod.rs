//! Per-result truncation at prompt assembly: tool results over a byte
//! cap ride the assembled prompt as their head plus a pointer stub,
//! while the session record and the stream keep every result verbatim.
//! The tail spills once to a deterministically named file under the
//! runner's temp mount — a resumed run re-materializes it from the
//! record alone — and a spill that cannot happen downgrades the stub
//! to a head without a pointer, never a failed run.

mod policy;

pub(crate) use policy::{Truncate, cap, resolve};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use craft_core::error::ProviderError;

/// The env knob bounding how many bytes of each tool result ride the
/// assembled prompt verbatim.
pub(crate) const RESULT_TRUNCATE_ENV: &str = "CRAFT_AGENT_RESULT_TRUNCATE_BYTES";

/// Result bytes kept when the knob is unset or empty.
pub(crate) const DEFAULT_RESULT_TRUNCATE_BYTES: u64 = 4096;

const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";

/// One env value into the cap: unset or empty keeps the default, zero
/// disables truncation, and anything unparseable is a typed error
/// naming the knob.
pub(crate) fn result_truncate_bytes(value: Option<&str>) -> Result<u64, ProviderError> {
    let Some(raw) = value.filter(|raw| !raw.is_empty()) else {
        return Ok(DEFAULT_RESULT_TRUNCATE_BYTES);
    };
    raw.trim().parse::<u64>().map_err(|_| invalid(raw))
}

/// The cap from the live process environment.
pub(crate) fn result_truncate_bytes_from_env() -> Result<u64, ProviderError> {
    match std::env::var(RESULT_TRUNCATE_ENV) {
        Ok(value) => result_truncate_bytes(Some(&value)),
        Err(std::env::VarError::NotPresent) => result_truncate_bytes(None),
        Err(err) => Err(invalid(&err.to_string())),
    }
}

fn invalid(raw: &str) -> ProviderError {
    ProviderError::InvalidRequest {
        detail: format!(
            "{RESULT_TRUNCATE_ENV} must be a whole number of bytes, 0 to disable truncation; got `{raw}`"
        ),
    }
}

/// The spill write root: the configured path, else a set RUNNER_TEMP,
/// else the platform temp dir. A policy input, never an error.
pub(crate) fn resolve_spill_root(config: Option<&Path>, runner_temp: Option<&str>) -> PathBuf {
    if let Some(path) = config {
        return path.to_path_buf();
    }
    if let Some(raw) = runner_temp.filter(|raw| !raw.is_empty()) {
        return PathBuf::from(raw);
    }
    std::env::temp_dir()
}

/// The spill root for a run, folding the live RUNNER_TEMP in.
pub(crate) fn spill_root_from_env(config: Option<PathBuf>) -> PathBuf {
    let runner_temp = std::env::var(RUNNER_TEMP_ENV).ok();
    resolve_spill_root(config.as_deref(), runner_temp.as_deref())
}

/// The spill directory stem: the session record's file stem, or a
/// process-unique name for in-memory runs, so two agents sharing a
/// temp mount never write the same spill name.
fn session_stem(session_path: Option<&Path>) -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    session_path
        .and_then(Path::file_stem)
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            format!("mem-{}-{seq}", std::process::id())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_knob_rows_follow_the_transport_parse_idiom() {
        assert_eq!(RESULT_TRUNCATE_ENV, "CRAFT_AGENT_RESULT_TRUNCATE_BYTES");
        assert_eq!(DEFAULT_RESULT_TRUNCATE_BYTES, 4096);
        assert_eq!(result_truncate_bytes(None), Ok(4096));
        assert_eq!(result_truncate_bytes(Some("")), Ok(4096));
        assert_eq!(result_truncate_bytes(Some(" 4096 ")), Ok(4096));
        assert_eq!(
            result_truncate_bytes(Some("0")),
            Ok(0),
            "zero disables truncation, matching the window knob"
        );
    }

    #[test]
    fn hostile_knob_values_surface_typed_errors_naming_the_knob() {
        for raw in ["garbage", "-5", "1.5", "1e3", "99999999999999999999"] {
            let err = result_truncate_bytes(Some(raw))
                .err()
                .unwrap_or_else(|| panic!("a hostile value must fail, not fall back: {raw}"));
            assert!(
                matches!(err, ProviderError::InvalidRequest { .. }),
                "got {err:?}"
            );
            assert!(err.to_string().contains(RESULT_TRUNCATE_ENV), "{err}");
            assert!(err.to_string().contains(raw), "{err}");
        }
    }

    #[test]
    fn root_resolution_prefers_config_then_env_then_platform_temp() {
        let config = PathBuf::from("/cfg/spill");
        assert_eq!(
            resolve_spill_root(Some(&config), Some("/runner/temp")),
            config
        );
        assert_eq!(
            resolve_spill_root(None, Some("/runner/temp")),
            PathBuf::from("/runner/temp")
        );
        assert_eq!(resolve_spill_root(None, Some("")), std::env::temp_dir());
        assert_eq!(resolve_spill_root(None, None), std::env::temp_dir());
    }
}
