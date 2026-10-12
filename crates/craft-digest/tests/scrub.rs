use craft_digest::scrub::scrub;

fn planted_secrets() -> Vec<(String, &'static str)> {
    vec![
        (format!("ghp_{}", "a".repeat(36)), "github classic"),
        (
            format!("github_pat_{}", "B".repeat(25)),
            "github fine-grained",
        ),
        (format!("gho_{}", "c".repeat(24)), "github oauth"),
        (format!("sk_live_{}", "d".repeat(14)), "stripe live"),
        (format!("rk_live_{}", "j".repeat(14)), "stripe restricted"),
        (format!("whsec_{}", "e".repeat(14)), "stripe webhook"),
        (format!("AKIA{}", "F0123456789ABCDEF"), "aws access key"),
        (format!("sk-{}", "f".repeat(24)), "openai classic"),
        (format!("sk-proj-{}", "g".repeat(22)), "openai project"),
        (format!("sk-ant-{}", "h".repeat(22)), "anthropic"),
        (format!("AIza{}", "i".repeat(24)), "google api key"),
        (
            format!("xox{}-{}", "b", "149695312899-953128-99abcdef0123"),
            "slack token",
        ),
        (
            format!("-----BEGIN {}PRIVATE KEY-----", "RSA "),
            "private key",
        ),
        (
            format!(
                "{}://{}:{}@{}",
                "postgres", "user", "hunter2", "db.internal"
            ),
            "dsn credential",
        ),
    ]
}

#[test]
fn planted_secrets_are_scrubbed() {
    for (secret, name) in planted_secrets() {
        let out = scrub(&format!("leaked value: {secret} in the digest body"));
        assert!(!out.contains(&secret), "kept {name}");
        assert!(out.contains("[scrubbed]"), "no marker for {name}");
    }
}

#[test]
fn legal_text_is_never_scrubbed() {
    let legal = "ask-me-anything about sk_livecoding at AKIAROSE in xoxing style";
    assert_eq!(scrub(legal), legal);
}

#[test]
fn scrub_works_across_lines() {
    let token = format!("ghp_{}", "z".repeat(40));
    let body = format!("# digest\n\nprose line\nsecret {token}\nmore prose\n");
    let out = scrub(&body);
    assert!(!out.contains(&token));
    assert!(out.contains("prose line"));
    assert!(out.contains("more prose"));
}

#[test]
fn pem_key_body_is_scrubbed_not_just_the_header() {
    let header = format!("-----BEGIN {}PRIVATE KEY-----", "RSA ");
    let key_line = "MIIEpAIBAAKCAQEA0123456789abcdef0123456789abcdef0123";
    let full = format!("{header}\n{key_line}\n{key_line}\n-----END RSA PRIVATE KEY-----\n");
    let out = scrub(&full);
    assert!(!out.contains(key_line), "{out}");
    assert!(out.contains("[scrubbed]"), "{out}");
}

#[test]
fn truncated_pem_body_without_the_end_line_is_scrubbed() {
    let header = format!("-----BEGIN {}PRIVATE KEY-----", "RSA ");
    let key_line = "MIIEpAIBAAKCAQEA0123456789abcdef0123456789abcdef0123";
    let out = scrub(&format!("{header}\n{key_line}\n{key_line}\n"));
    assert!(!out.contains(key_line), "{out}");
    assert!(out.contains("[scrubbed]"), "{out}");
}

#[test]
fn slack_token_body_is_scrubbed_not_just_the_prefix() {
    let token_body = "149695312899-953128-99abcdef012345";
    let token = format!("xox{}-{}", "b", token_body);
    let out = scrub(&format!("leaked {token} in prose"));
    assert!(!out.contains(token_body), "{out}");
    assert!(out.contains("[scrubbed]"), "{out}");
}
