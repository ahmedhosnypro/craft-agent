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
        (concat!("xox", "b", "-123").to_string(), "slack token"),
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
