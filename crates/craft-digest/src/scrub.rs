//! Secret scrubbing: digests never leave the binary carrying a credential.
//! The families come from scripts/scan-secrets.sh; where its detection
//! prefix stops, these rules redact the whole credential.

use std::sync::OnceLock;

use regex::Regex;

const SCRUBBED: &str = "[scrubbed]";
const BOUNDARY_SCRUBBED: &str = "${1}[scrubbed]";

struct Rule {
    pattern: &'static str,
    template: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        // full armored block, header through the END line
        pattern: r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
        template: SCRUBBED,
    },
    Rule {
        // truncated block: header plus base64-shaped lines, no END line
        pattern: r"-----BEGIN [A-Z ]*PRIVATE KEY-----\n(?:[A-Za-z0-9+/=]{20,}\n?)+",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"sk_live_[A-Za-z0-9]{10,}",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"rk_live_[A-Za-z0-9]{10,}",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"AKIA[0-9A-Z]{16}",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"ghp_[A-Za-z0-9]{30,}",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"github_pat_[A-Za-z0-9_]{20,}",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"gh[os]_[A-Za-z0-9]{20,}",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"xox[abpr]-[A-Za-z0-9-]{10,}",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"(?m)(^|[^A-Za-z0-9])sk-[A-Za-z0-9]{20,}",
        template: BOUNDARY_SCRUBBED,
    },
    Rule {
        pattern: r"sk-proj-[A-Za-z0-9_-]{20,}",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"sk-ant-[A-Za-z0-9_-]{20,}",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"AIza[A-Za-z0-9_-]{20,}",
        template: SCRUBBED,
    },
    Rule {
        pattern: r"whsec_[A-Za-z0-9]{10,}",
        template: SCRUBBED,
    },
    Rule {
        pattern: concat!(r#"[a-z][a-z0-9+.-]*://[^/"' ]+:[^/"' @]+"#, "@",),
        template: SCRUBBED,
    },
];

fn compiled() -> &'static Vec<(Regex, &'static str)> {
    static COMPILED: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        RULES
            .iter()
            .filter_map(|rule| Regex::new(rule.pattern).ok().map(|re| (re, rule.template)))
            .collect()
    })
}

/// Replace every secret-shaped substring with a scrub marker.
pub fn scrub(text: &str) -> String {
    let mut out = text.to_string();
    for (regex, template) in compiled() {
        out = regex.replace_all(&out, *template).into_owned();
    }
    out
}
