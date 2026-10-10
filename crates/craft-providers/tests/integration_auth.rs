//! Live bad-key auth corpus for the provider seam, driven by the
//! repo-root `.env` (`TEST_BYOK_*`); runs via `just integration_auth`.

#[test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
fn integration_auth() {}
