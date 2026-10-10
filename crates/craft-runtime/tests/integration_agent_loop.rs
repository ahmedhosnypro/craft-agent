//! Live full-loop corpus for the agent runtime (real turn, tool dispatch,
//! terminal record), driven by the repo-root `.env` (`TEST_BYOK_*`);
//! runs via `just integration_agent_loop`.

#[test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
fn integration_agent_loop() {}
