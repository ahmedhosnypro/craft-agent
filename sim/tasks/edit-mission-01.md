# Mission: one precise edit to an existing file, then a pull request

You are craft-agent running in a runner simulation. This repository is
craft-agent itself. Make exactly one small, precisely specified edit
to one existing tracked file — the docs index in README.md — then open
a pull request carrying it. The edit_file tool does the change: read
the file, replace one exact line, touch nothing else.

## The exact expected change

The `## Docs` section of README.md lists the numbered document series
and ends with this exact line:

    [06 testing](docs/06-testing.md)

Two tracked documents are missing from that index: the release guide
(`docs/release.md`) and the smoke checklist
(`docs/smoke-checklist.md`). Replace that one line with these three:

    [06 testing](docs/06-testing.md) ·
    [release guide](docs/release.md) ·
    [smoke checklist](docs/smoke-checklist.md)

The change adds the two links; every other line of the file stays
byte-identical.

## How to make it

- Read README.md first: edit_file replaces an exact byte match in a
  previously read file and aborts if the file changed on disk.
- One edit_file call: the old text is exactly
  `[06 testing](docs/06-testing.md)`; the new text is exactly the three
  lines above. The match is unique in the file; if it is missing or
  ambiguous, stop and report — never rewrite the file.
- Verify before committing; both commands must print `1`:
  `grep -c '\[release guide\](docs/release.md)' README.md` and
  `grep -c '\[smoke checklist\](docs/smoke-checklist.md)' README.md`.

## Rules (absolute)

- Work only inside this workspace. Never touch any other repository,
  and never call gh against the storytold organization.
- One file, one edit: README.md only, the single replacement above —
  no reformatting, no rewording, no new files.
- Never print environment variables or secrets.

## Deliver

- Create the branch `agent/edit-mission-01`, commit with the
  conventional commit message `docs(readme): link the release guide
  and smoke checklist`, and stage explicit paths only — never
  `git add -A`.
- Push the branch and open a pull request to THIS repository titled
  `docs(readme): link the release guide and smoke checklist`, with a
  body naming the two added index entries.
- Keep the diff minimal: the one-file edit, nothing else.
