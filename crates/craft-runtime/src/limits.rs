//! Context-limit and thinking-level knobs: the env names, hostile-value
//! parsing (typed errors naming the knob), and the resolution precedence
//! CLI flag > agent frontmatter > env var > built-in default.

use craft_core::agents::AgentFile;

use crate::runner::RunConfig;

/// Env knob for the context window cap in tokens.
pub const ENV_MAX_CONTEXT_TOKENS: &str = "CRAFT_AGENT_MAX_CONTEXT_TOKENS";
/// Env knob for the input-side compaction trigger limit in tokens.
pub const ENV_MAX_INPUT_TOKENS: &str = "CRAFT_AGENT_MAX_INPUT_TOKENS";
/// Env knob for the per-request output cap in tokens.
pub const ENV_MAX_OUTPUT_TOKENS: &str = "CRAFT_AGENT_MAX_OUTPUT_TOKENS";
/// Env knob for the thinking level string.
pub const ENV_THINKING_LEVEL: &str = "CRAFT_AGENT_THINKING_LEVEL";

/// Hostile-input bound on the thinking-level string.
pub const THINKING_LEVEL_BYTE_CAP: usize = 128;

/// The four resolved knobs; None keeps the built-in default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextKnobs {
    pub max_context_tokens: Option<u64>,
    pub max_input_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub thinking_level: Option<String>,
}

impl ContextKnobs {
    /// The four env knobs from a lookup; a hostile value is a typed
    /// error naming its knob.
    pub fn from_env(read: &dyn Fn(&str) -> Option<String>) -> Result<Self, String> {
        Ok(Self {
            max_context_tokens: token_knob(ENV_MAX_CONTEXT_TOKENS, read(ENV_MAX_CONTEXT_TOKENS))?,
            max_input_tokens: token_knob(ENV_MAX_INPUT_TOKENS, read(ENV_MAX_INPUT_TOKENS))?,
            max_output_tokens: token_knob(ENV_MAX_OUTPUT_TOKENS, read(ENV_MAX_OUTPUT_TOKENS))?,
            thinking_level: thinking_knob(read(ENV_THINKING_LEVEL))?,
        })
    }

    /// The knobs an agent profile's frontmatter carries.
    pub fn from_agent(agent: Option<&AgentFile>) -> Self {
        let Some(agent) = agent else {
            return Self::default();
        };
        Self {
            max_context_tokens: agent.max_context_tokens,
            max_input_tokens: agent.max_input_tokens,
            max_output_tokens: agent.max_output_tokens,
            thinking_level: agent.thinking.clone(),
        }
    }

    /// Per knob: CLI flag > frontmatter > env var.
    pub fn resolve(cli: &Self, frontmatter: &Self, env: &Self) -> Self {
        Self {
            max_context_tokens: pick(
                cli.max_context_tokens,
                frontmatter.max_context_tokens,
                env.max_context_tokens,
            ),
            max_input_tokens: pick(
                cli.max_input_tokens,
                frontmatter.max_input_tokens,
                env.max_input_tokens,
            ),
            max_output_tokens: pick(
                cli.max_output_tokens,
                frontmatter.max_output_tokens,
                env.max_output_tokens,
            ),
            thinking_level: cli
                .thinking_level
                .clone()
                .or(frontmatter.thinking_level.clone())
                .or(env.thinking_level.clone()),
        }
    }
}

/// CLI flag wins, then frontmatter, then env.
fn pick(cli: Option<u64>, frontmatter: Option<u64>, env: Option<u64>) -> Option<u64> {
    cli.or(frontmatter).or(env)
}

impl ContextKnobs {
    /// The knobs are valid: the output cap fits the 32-bit wire field.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(cap) = self.max_output_tokens
            && u32::try_from(cap).is_err()
        {
            return Err(format!(
                "the max output tokens knob must be at most {}; got {cap}",
                u32::MAX
            ));
        }
        Ok(())
    }

    /// Folds the resolved knobs into a run config; the output cap maps
    /// onto the per-request `max_tokens` (provider ceilings still
    /// clamp downstream).
    pub fn apply(&self, config: &mut RunConfig) -> Result<(), String> {
        self.validate()?;
        config.max_context_tokens = self.max_context_tokens;
        config.max_input_tokens = self.max_input_tokens;
        config.max_output_tokens = self.max_output_tokens;
        config.thinking_level = self.thinking_level.clone();
        if let Some(cap) = self.max_output_tokens {
            config.max_tokens = u32::try_from(cap).map_err(|_| {
                String::from("the max output tokens knob exceeds the 32-bit wire cap")
            })?;
        }
        Ok(())
    }
}

/// One token knob: unset or empty stays unset, a whole number of at
/// least 1 passes, anything else is a typed error naming the knob.
pub fn token_knob(knob: &str, raw: Option<String>) -> Result<Option<u64>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let invalid = || format!("{knob} must be a whole number of at least 1; got `{trimmed}`");
    let value: u64 = trimmed.parse().map_err(|_| invalid())?;
    if value == 0 {
        return Err(invalid());
    }
    Ok(Some(value))
}

/// The thinking-level knob: unset or empty stays unset, any other
/// string passes through verbatim under the byte cap.
pub fn thinking_knob(raw: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > THINKING_LEVEL_BYTE_CAP {
        return Err(format!(
            "{ENV_THINKING_LEVEL} exceeds the {THINKING_LEVEL_BYTE_CAP}-byte cap"
        ));
    }
    Ok(Some(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_map(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn env_lookup(
        map: &std::collections::HashMap<String, String>,
    ) -> impl Fn(&str) -> Option<String> + '_ {
        move |name| map.get(name).cloned()
    }

    fn knobs(max_context: Option<u64>, thinking: Option<&str>) -> ContextKnobs {
        ContextKnobs {
            max_context_tokens: max_context,
            max_input_tokens: None,
            max_output_tokens: None,
            thinking_level: thinking.map(String::from),
        }
    }

    #[test]
    fn precedence_is_cli_then_frontmatter_then_env_per_knob() {
        let cli = ContextKnobs {
            max_context_tokens: Some(1),
            max_input_tokens: Some(2),
            max_output_tokens: Some(3),
            thinking_level: Some(String::from("high")),
        };
        let frontmatter = ContextKnobs {
            max_context_tokens: Some(10),
            max_input_tokens: Some(20),
            max_output_tokens: Some(30),
            thinking_level: Some(String::from("low")),
        };
        let env = ContextKnobs {
            max_context_tokens: Some(100),
            max_input_tokens: Some(200),
            max_output_tokens: Some(300),
            thinking_level: Some(String::from("max")),
        };
        let resolved = ContextKnobs::resolve(&cli, &frontmatter, &env);
        assert_eq!(resolved.max_context_tokens, Some(1));
        assert_eq!(resolved.max_input_tokens, Some(2));
        assert_eq!(resolved.max_output_tokens, Some(3));
        assert_eq!(resolved.thinking_level.as_deref(), Some("high"));
        let resolved = ContextKnobs::resolve(&ContextKnobs::default(), &frontmatter, &env);
        assert_eq!(resolved.max_context_tokens, Some(10));
        assert_eq!(resolved.thinking_level.as_deref(), Some("low"));
        let resolved =
            ContextKnobs::resolve(&ContextKnobs::default(), &ContextKnobs::default(), &env);
        assert_eq!(resolved.max_context_tokens, Some(100));
        assert_eq!(resolved.max_input_tokens, Some(200));
        assert_eq!(resolved.max_output_tokens, Some(300));
        assert_eq!(resolved.thinking_level.as_deref(), Some("max"));
        let resolved = ContextKnobs::resolve(
            &ContextKnobs::default(),
            &ContextKnobs::default(),
            &ContextKnobs::default(),
        );
        assert_eq!(resolved, ContextKnobs::default());
    }

    #[test]
    fn from_env_reads_all_four_knobs_and_fails_closed_naming_the_knob() {
        let env = env_map(&[
            (ENV_MAX_CONTEXT_TOKENS, "1000000"),
            (ENV_MAX_INPUT_TOKENS, "800000"),
            (ENV_MAX_OUTPUT_TOKENS, "65536"),
            (ENV_THINKING_LEVEL, "high"),
        ]);
        let knobs = ContextKnobs::from_env(&env_lookup(&env)).expect("clean env parses");
        assert_eq!(knobs.max_context_tokens, Some(1_000_000));
        assert_eq!(knobs.max_input_tokens, Some(800_000));
        assert_eq!(knobs.max_output_tokens, Some(65_536));
        assert_eq!(knobs.thinking_level.as_deref(), Some("high"));
        let env = env_map(&[(ENV_MAX_CONTEXT_TOKENS, "garbage")]);
        let err = ContextKnobs::from_env(&env_lookup(&env)).expect_err("hostile env fails closed");
        assert!(
            err.contains(ENV_MAX_CONTEXT_TOKENS),
            "names the knob: {err}"
        );
        let oversized = "x".repeat(THINKING_LEVEL_BYTE_CAP + 1);
        let env = env_map(&[(ENV_THINKING_LEVEL, oversized.as_str())]);
        let err = ContextKnobs::from_env(&env_lookup(&env)).expect_err("hostile env fails closed");
        assert!(err.contains(ENV_THINKING_LEVEL), "names the knob: {err}");
    }

    #[test]
    fn token_knob_rejects_garbage_zero_and_overflow_naming_the_knob() {
        assert_eq!(token_knob("K", None), Ok(None));
        assert_eq!(token_knob("K", Some(String::new())), Ok(None));
        assert_eq!(token_knob("K", Some(String::from("  "))), Ok(None));
        assert_eq!(token_knob("K", Some(String::from("42"))), Ok(Some(42)));
        assert_eq!(
            token_knob("K", Some(u64::MAX.to_string())),
            Ok(Some(u64::MAX))
        );
        for hostile in ["abc", "0", "-1", "1.5", "99999999999999999999999999"] {
            let err = token_knob(
                "CRAFT_AGENT_MAX_CONTEXT_TOKENS",
                Some(String::from(hostile)),
            )
            .expect_err("hostile value must fail closed");
            assert!(
                err.contains("CRAFT_AGENT_MAX_CONTEXT_TOKENS"),
                "error names the knob: {err}"
            );
        }
    }

    #[test]
    fn thinking_knob_passes_arbitrary_strings_and_caps_hostile_ones() {
        assert_eq!(thinking_knob(None), Ok(None));
        assert_eq!(thinking_knob(Some(String::new())), Ok(None));
        assert_eq!(
            thinking_knob(Some(String::from("custom-verbosity"))),
            Ok(Some(String::from("custom-verbosity")))
        );
        let oversized = "x".repeat(THINKING_LEVEL_BYTE_CAP + 1);
        let err = thinking_knob(Some(oversized)).expect_err("oversized level fails closed");
        assert!(
            err.contains(ENV_THINKING_LEVEL),
            "error names the knob: {err}"
        );
    }

    #[test]
    fn apply_maps_output_cap_onto_max_tokens_and_rejects_wide_caps() {
        let mut config = RunConfig::new(std::path::PathBuf::from("/tmp"));
        let knobs = ContextKnobs {
            max_output_tokens: Some(4096),
            thinking_level: Some(String::from("medium")),
            ..knobs(Some(1_000_000), Some("medium"))
        };
        knobs.apply(&mut config).expect("valid knobs apply");
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.max_context_tokens, Some(1_000_000));
        assert_eq!(config.thinking_level.as_deref(), Some("medium"));
        let wide = ContextKnobs {
            max_output_tokens: Some(u64::from(u32::MAX) + 1),
            ..ContextKnobs::default()
        };
        assert!(wide.validate().is_err());
        assert!(wide.apply(&mut config).is_err());
    }
}
