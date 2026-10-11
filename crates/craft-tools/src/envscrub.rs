//! Child-env scrubbing: provider-key variables never reach tool
//! children, while GH_TOKEN and everything unrelated pass through.
//! Applied per call from the context's env source — never cached.

/// True for variables a tool child must never see: the provider key
/// names, the whole `TEST_BYOK_*` family, and any `*_API_KEY` key.
fn is_provider_key(name: &str) -> bool {
    name == "API_KEY" || name.ends_with("_API_KEY") || name.starts_with("TEST_BYOK_")
}

/// Filter an environment down to what a tool child may receive. Order and
/// values are preserved.
pub fn scrub(env: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    env.into_iter()
        .filter(|(k, _)| !is_provider_key(k))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::scrub;

    #[test]
    fn removes_key_families_and_keeps_the_rest_in_order() {
        let kept = scrub(vec![
            ("ZED".to_string(), "1".to_string()),
            ("OPENAI_API_KEY".to_string(), "x".to_string()),
            ("GH_TOKEN".to_string(), "tok".to_string()),
            ("TEST_BYOK_BASE_URL".to_string(), "u".to_string()),
        ]);
        assert_eq!(
            kept,
            vec![
                ("ZED".to_string(), "1".to_string()),
                ("GH_TOKEN".to_string(), "tok".to_string()),
            ]
        );
    }
}
