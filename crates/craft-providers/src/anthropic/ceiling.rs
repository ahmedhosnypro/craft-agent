//! Per-family max_tokens ceilings: the Messages API rejects values
//! above each model family's output cap.

/// Which family a model id names.
#[derive(Clone, Copy)]
enum Family {
    Opus,
    Sonnet,
    Haiku,
}

/// The generation a model id carries; a bare family id aliases the
/// current generation.
enum Version {
    Bare,
    Number { major: u32, minor: Option<u32> },
}

/// The output-token cap for a model id; unknown ids carry none.
pub(super) fn ceiling(model: &str) -> Option<u32> {
    match (family_of(model)?, version_of(model)) {
        // opus: 3.x caps at 4096, 4.0-4.1 at 32000, 4.5 at 64000, 4.6+ at 128000.
        (Family::Opus, Version::Bare) => Some(128_000),
        (Family::Opus, Version::Number { major: 3, .. }) => Some(4_096),
        (Family::Opus, Version::Number { major: 4, minor }) if minor.unwrap_or(0) < 5 => {
            Some(32_000)
        }
        (
            Family::Opus,
            Version::Number {
                major: 4,
                minor: Some(5),
            },
        ) => Some(64_000),
        (Family::Opus, Version::Number { major: 4, .. }) => Some(128_000),
        (Family::Opus, Version::Number { major, .. }) if major >= 5 => Some(128_000),
        // sonnet: 3.x caps at 8192, 4.0-4.5 at 64000, 4.6+ at 128000.
        (Family::Sonnet, Version::Bare) => Some(128_000),
        (Family::Sonnet, Version::Number { major: 3, .. }) => Some(8_192),
        (Family::Sonnet, Version::Number { major, minor })
            if (major, minor.unwrap_or(0)) >= (4, 6) =>
        {
            Some(128_000)
        }
        (Family::Sonnet, Version::Number { major, .. }) if major >= 4 => Some(64_000),
        // haiku: 3.0 caps at 4096, 3.5 at 8192, 4+ at 64000.
        (Family::Haiku, Version::Bare) => Some(64_000),
        (
            Family::Haiku,
            Version::Number {
                major: 3,
                minor: Some(5),
            },
        ) => Some(8_192),
        (Family::Haiku, Version::Number { major: 3, .. }) => Some(4_096),
        (Family::Haiku, Version::Number { major, .. }) if major >= 4 => Some(64_000),
        _ => None,
    }
}

/// The wire max_tokens: clamped to the family ceiling when one is
/// known, passed through untouched otherwise.
pub(super) fn effective_max_tokens(model: &str, requested: u32) -> u32 {
    match ceiling(model) {
        Some(cap) => requested.min(cap),
        None => requested,
    }
}

/// The first family token in the id, case-insensitive.
fn family_of(model: &str) -> Option<Family> {
    model.split('-').find_map(|token| {
        if token.eq_ignore_ascii_case("opus") {
            Some(Family::Opus)
        } else if token.eq_ignore_ascii_case("sonnet") {
            Some(Family::Sonnet)
        } else if token.eq_ignore_ascii_case("haiku") {
            Some(Family::Haiku)
        } else {
            None
        }
    })
}

/// The first two numeric tokens become major and minor.
fn version_of(model: &str) -> Version {
    let mut numbers = model.split('-').filter_map(number);
    match (numbers.next(), numbers.next()) {
        (Some(major), minor) => Version::Number { major, minor },
        (None, _) => Version::Bare,
    }
}

fn number(token: &str) -> Option<u32> {
    if !token.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}
