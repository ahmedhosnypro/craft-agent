//! Live dialect-mount corpus: the Anthropic-messages dialect mounted over
//! the same base URL, driven by the repo-root `.env` (`TEST_BYOK_*`);
//! runs via `just integration_dialect_mount`.

#[test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
fn integration_dialect_mount() {}
