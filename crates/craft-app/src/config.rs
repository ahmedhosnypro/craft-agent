//! Env resolution: a named provider, a named key, no ambient fallbacks.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;

use craft_providers::anthropic::AnthropicProvider;
use craft_providers::backend::Backend;
use craft_providers::gemini::GeminiProvider;
use craft_providers::openai::OpenAiProvider;

use crate::error::AppError;

pub type EnvMap = HashMap<String, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Gemini,
}

/// The resolved launch configuration: which backend dialect to mount and
/// the credentials it needs. Values come only from explicitly named env
/// vars — a missing or empty variable is a hard error naming itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub gh_token: Option<String>,
}

pub fn env_from_process() -> EnvMap {
    collect_env(std::env::vars_os())
}

/// Non-UTF-8 entries are dropped, never a panic; a variable the config
/// actually needs then surfaces as its named missing-env error.
fn collect_env<I>(vars: I) -> EnvMap
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    vars.into_iter()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

fn require(env: &EnvMap, var: &'static str) -> Result<String, AppError> {
    match env.get(var) {
        Some(value) if !value.is_empty() => Ok(value.clone()),
        _ => Err(AppError::MissingEnv { var }),
    }
}

fn optional(env: &EnvMap, var: &'static str) -> Option<String> {
    let value = env.get(var)?;
    if value.is_empty() {
        return None;
    }
    Some(value.clone())
}

pub fn resolve_env_config(env: &EnvMap) -> Result<EnvConfig, AppError> {
    let provider = require(env, "CRAFT_AGENT_PROVIDER")?;
    let provider = match provider.as_str() {
        "openai" => ProviderKind::OpenAi,
        "anthropic" => ProviderKind::Anthropic,
        "gemini" => ProviderKind::Gemini,
        other => {
            return Err(AppError::InvalidProvider {
                value: other.to_string(),
            });
        }
    };
    let model = require(env, "CRAFT_AGENT_MODEL")?;
    let (key_var, default_base) = match provider {
        ProviderKind::OpenAi => ("OPENAI_API_KEY", craft_providers::openai::DEFAULT_BASE_URL),
        ProviderKind::Anthropic => (
            "ANTHROPIC_API_KEY",
            craft_providers::anthropic::DEFAULT_BASE_URL,
        ),
        ProviderKind::Gemini => ("GOOGLE_API_KEY", craft_providers::gemini::DEFAULT_BASE_URL),
    };
    let api_key = require(env, key_var)?;
    let base_url = match provider {
        ProviderKind::OpenAi => {
            let override_url = optional(env, "OPENAI_BASE_URL");
            override_url.unwrap_or_else(|| default_base.to_string())
        }
        _ => default_base.to_string(),
    };
    Ok(EnvConfig {
        provider,
        model,
        api_key,
        base_url,
        gh_token: optional(env, "GH_TOKEN"),
    })
}

pub fn mount_backend(config: &EnvConfig) -> Result<Arc<Backend>, AppError> {
    let backend = match config.provider {
        ProviderKind::OpenAi => {
            let provider = OpenAiProvider::new(&config.base_url, &config.model, &config.api_key)
                .map_err(|source| AppError::ProviderInit {
                    name: "openai",
                    source,
                })?;
            Backend::OpenAi(provider)
        }
        ProviderKind::Anthropic => {
            let provider = AnthropicProvider::new(&config.base_url, &config.model, &config.api_key)
                .map_err(|source| AppError::ProviderInit {
                    name: "anthropic",
                    source,
                })?;
            Backend::Anthropic(provider)
        }
        ProviderKind::Gemini => {
            let provider = GeminiProvider::new(&config.base_url, &config.model, &config.api_key)
                .map_err(|source| AppError::ProviderInit {
                    name: "gemini",
                    source,
                })?;
            Backend::Gemini(provider)
        }
    };
    Ok(Arc::new(backend))
}

#[cfg(test)]
mod tests {
    use super::collect_env;
    use std::ffi::OsString;

    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;

    #[test]
    #[cfg(unix)]
    fn non_utf8_env_entries_are_skipped_not_panics() {
        let vars = vec![
            (OsString::from("OK"), OsString::from("1")),
            (OsString::from_vec(vec![0xff, 0xfe]), OsString::from("2")),
            (OsString::from("ALSO_OK"), OsString::from_vec(vec![0xff])),
        ];
        let map = collect_env(vars);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("OK").map(String::as_str), Some("1"));
    }
}
