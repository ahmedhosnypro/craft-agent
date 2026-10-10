# 04 — The digest mission

`craft-agent digest --since <window> --org storytold`:

1. **Collect** (deterministic, keyless): merged PRs per repository via
   `gh pr list --state merged` (looped per repo to dodge search caps),
   diff stats, and duplicate-code candidates from a copy-paste detector
   driven over the suite sources.
2. **Summarize** (one bounded agent turn): a strict digest prompt over the
   collected bundle, with read/bash/gh tools available so every claim is
   verified against real data; each target repository's AGENTS.md is
   loaded as binding context; only measured facts, every claim linked to
   its PR or file.
3. **Publish** (two-job split): the agent job (read access + provider key,
   no issue writing) produces the digest artifact; the publish job (App
   token, issue writing, no provider key) creates or updates the
   suite-digest sticky issue, located by a title marker plus a state file.
