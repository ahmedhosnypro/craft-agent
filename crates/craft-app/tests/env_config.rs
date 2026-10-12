//! RED corpus: env resolution — fixture env maps, never a live `.env`.

use std::collections::HashMap;

use craft_app::AppError;
use craft_app::config::{EnvMap, ProviderKind, mount_backend, resolve_env_config};
use craft_providers::anthropic::AuthScheme;

fn env(pairs: &[(&str, &str)]) -> EnvMap {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect::<HashMap<String, String>>()
}

#[test]
fn missing_provider_is_a_named_error() {
    let e = env(&[("CRAFT_AGENT_MODEL", "gpt-4o")]);
    let err = resolve_env_config(&e).expect_err("provider must be named");
    assert!(matches!(
        err,
        AppError::MissingEnv {
            var: "CRAFT_AGENT_PROVIDER"
        }
    ));
    assert!(err.to_string().contains("CRAFT_AGENT_PROVIDER"));
}

#[test]
fn invalid_provider_value_is_rejected() {
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "claude"),
        ("CRAFT_AGENT_MODEL", "m"),
    ]);
    let err = resolve_env_config(&e).expect_err("unknown provider must fail");
    assert!(matches!(err, AppError::InvalidProvider { ref value } if value == "claude"));
    assert!(err.to_string().contains("anthropic"));
    assert!(err.to_string().contains("openai"));
    assert!(err.to_string().contains("gemini"));
}

#[test]
fn missing_model_is_a_named_error() {
    let e = env(&[("CRAFT_AGENT_PROVIDER", "openai"), ("OPENAI_API_KEY", "k")]);
    let err = resolve_env_config(&e).expect_err("model must be required");
    assert!(matches!(
        err,
        AppError::MissingEnv {
            var: "CRAFT_AGENT_MODEL"
        }
    ));
    assert!(err.to_string().contains("CRAFT_AGENT_MODEL"));
}

#[test]
fn openai_resolves_with_default_base_url() {
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "openai"),
        ("CRAFT_AGENT_MODEL", "gpt-4o-mini"),
        ("OPENAI_API_KEY", "sk-test"),
    ]);
    let cfg = resolve_env_config(&e).expect("openai resolves");
    assert_eq!(cfg.provider, ProviderKind::OpenAi);
    assert_eq!(cfg.model, "gpt-4o-mini");
    assert_eq!(cfg.api_key, "sk-test");
    assert_eq!(cfg.base_url, "https://api.openai.com/v1");
    assert_eq!(cfg.gh_token, None);
}

#[test]
fn openai_base_url_env_overrides_default() {
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "openai"),
        ("CRAFT_AGENT_MODEL", "m"),
        ("OPENAI_API_KEY", "k"),
        ("OPENAI_BASE_URL", "http://localhost:8080/v1"),
    ]);
    let cfg = resolve_env_config(&e).expect("openai resolves with custom base url");
    assert_eq!(cfg.base_url, "http://localhost:8080/v1");
}

#[test]
fn anthropic_resolves_with_its_key() {
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "anthropic"),
        ("CRAFT_AGENT_MODEL", "claude-sonnet-4"),
        ("ANTHROPIC_API_KEY", "ak-test"),
    ]);
    let cfg = resolve_env_config(&e).expect("anthropic resolves");
    assert_eq!(cfg.provider, ProviderKind::Anthropic);
    assert_eq!(cfg.api_key, "ak-test");
    assert_eq!(cfg.base_url, "https://api.anthropic.com");
    assert_eq!(cfg.anthropic_auth, AuthScheme::ApiKey);
}

#[test]
fn anthropic_base_url_env_overrides_default() {
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "anthropic"),
        ("CRAFT_AGENT_MODEL", "m"),
        ("ANTHROPIC_API_KEY", "k"),
        ("ANTHROPIC_BASE_URL", "http://localhost:8080"),
    ]);
    let cfg = resolve_env_config(&e).expect("anthropic resolves with custom base url");
    assert_eq!(cfg.base_url, "http://localhost:8080");
}

#[test]
fn anthropic_auth_bearer_mounts_the_bearer_scheme() {
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "anthropic"),
        ("CRAFT_AGENT_MODEL", "m"),
        ("ANTHROPIC_API_KEY", "k"),
        ("ANTHROPIC_AUTH", "bearer"),
    ]);
    let cfg = resolve_env_config(&e).expect("bearer is a legal knob value");
    assert_eq!(cfg.anthropic_auth, AuthScheme::Bearer);
}

#[test]
fn anthropic_auth_x_api_key_or_unset_defaults_to_the_header_scheme() {
    for value in ["x-api-key", ""] {
        let e = env(&[
            ("CRAFT_AGENT_PROVIDER", "anthropic"),
            ("CRAFT_AGENT_MODEL", "m"),
            ("ANTHROPIC_API_KEY", "k"),
            ("ANTHROPIC_AUTH", value),
        ]);
        let cfg = resolve_env_config(&e).expect("x-api-key and empty keep the default");
        assert_eq!(cfg.anthropic_auth, AuthScheme::ApiKey);
    }
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "anthropic"),
        ("CRAFT_AGENT_MODEL", "m"),
        ("ANTHROPIC_API_KEY", "k"),
    ]);
    let cfg = resolve_env_config(&e).expect("unset keeps the default");
    assert_eq!(cfg.anthropic_auth, AuthScheme::ApiKey);
}

#[test]
fn anthropic_auth_unknown_value_is_a_named_error() {
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "anthropic"),
        ("CRAFT_AGENT_MODEL", "m"),
        ("ANTHROPIC_API_KEY", "k"),
        ("ANTHROPIC_AUTH", "token"),
    ]);
    let err = resolve_env_config(&e).expect_err("an unknown scheme must fail");
    assert!(matches!(err, AppError::InvalidAuth { ref value } if value == "token"));
    assert!(err.to_string().contains("ANTHROPIC_AUTH"));
    assert!(err.to_string().contains("token"));
}

#[test]
fn gemini_resolves_with_its_key() {
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "gemini"),
        ("CRAFT_AGENT_MODEL", "gemini-2"),
        ("GOOGLE_API_KEY", "gk-test"),
    ]);
    let cfg = resolve_env_config(&e).expect("gemini resolves");
    assert_eq!(cfg.provider, ProviderKind::Gemini);
    assert_eq!(cfg.api_key, "gk-test");
    assert_eq!(cfg.base_url, "https://generativelanguage.googleapis.com");
}

#[test]
fn named_provider_with_missing_key_names_the_variable() {
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "anthropic"),
        ("CRAFT_AGENT_MODEL", "m"),
    ]);
    let err = resolve_env_config(&e).expect_err("anthropic key must be required");
    assert!(matches!(
        err,
        AppError::MissingEnv {
            var: "ANTHROPIC_API_KEY"
        }
    ));
    assert!(err.to_string().contains("ANTHROPIC_API_KEY"));
}

#[test]
fn no_ambient_fallback_across_providers() {
    // provider says gemini; only OPENAI_API_KEY is set — must still name
    // GOOGLE_API_KEY, never silently adopt the other key.
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "gemini"),
        ("CRAFT_AGENT_MODEL", "m"),
        ("OPENAI_API_KEY", "sk-test"),
    ]);
    let err = resolve_env_config(&e).expect_err("no cross-provider fallback");
    assert!(matches!(
        err,
        AppError::MissingEnv {
            var: "GOOGLE_API_KEY"
        }
    ));
}

#[test]
fn gh_token_passes_through_when_present() {
    let e = env(&[
        ("CRAFT_AGENT_PROVIDER", "openai"),
        ("CRAFT_AGENT_MODEL", "m"),
        ("OPENAI_API_KEY", "k"),
        ("GH_TOKEN", "ghp-test"),
    ]);
    let cfg = resolve_env_config(&e).expect("openai resolves");
    assert_eq!(cfg.gh_token.as_deref(), Some("ghp-test"));
}

#[test]
fn backend_mounts_for_each_resolved_provider() {
    for provider in ["openai", "anthropic", "gemini"] {
        let key_var = match provider {
            "openai" => "OPENAI_API_KEY",
            "anthropic" => "ANTHROPIC_API_KEY",
            _ => "GOOGLE_API_KEY",
        };
        let e = env(&[
            ("CRAFT_AGENT_PROVIDER", provider),
            ("CRAFT_AGENT_MODEL", "some-model"),
            (key_var, "fixture-key"),
        ]);
        let cfg = resolve_env_config(&e).expect("provider resolves");
        assert!(
            mount_backend(&cfg).is_ok(),
            "backend mounts for provider `{provider}`"
        );
    }
}
