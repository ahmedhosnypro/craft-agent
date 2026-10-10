PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS tasks (
  id INTEGER PRIMARY KEY,
  slug TEXT UNIQUE NOT NULL,
  title TEXT,
  category TEXT,
  status TEXT NOT NULL DEFAULT 'pending',
  output_file TEXT,
  started_at TEXT,
  finished_at TEXT,
  notes TEXT
);
CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_slug TEXT,
  event TEXT,
  detail TEXT,
  created_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS findings (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_slug TEXT,
  topic TEXT,
  detail TEXT,
  source TEXT,
  created_at TEXT DEFAULT (datetime('now'))
);
DELETE FROM tasks;
INSERT INTO tasks (slug,title,category,status,output_file) VALUES
 ('ex1-filmcraft-pr-context','Filmcraft PR 553 comment + repo context','local','pending','analysis/01-filmcraft-pr-context.md'),
 ('ex2-storytold-org-scan','storytold org repos scan (workflows, bot activity, shared infra)','local','pending','analysis/02-storytold-org-scan.md'),
 ('ex3-photocraft-local','photocraft local repo (settings toggles, automation, control protocol)','local','pending','analysis/03-photocraft-local.md'),
 ('ex4-harhaial-bf1','hikal_ai harhaial-backend .worktrees/bf1 exploration','local','pending','analysis/04-harhaial-backend-bf1.md'),
 ('ex5-kimi-code','kimi-code project (headless agent CLI patterns, BYOK)','local','pending','analysis/05-kimi-code.md'),
 ('ex6-deepseek-harness','deepseek-harness project exploration','local','pending','analysis/06-deepseek-harness.md'),
 ('ex7-grok-build','grok-build project exploration','local','pending','analysis/07-grok-build.md'),
 ('ws1-public-agentic-repos','Websearch: public repos with agentic AI GitHub Action solutions','websearch','pending','analysis/10-websearch-public-agentic-repos.md'),
 ('ws2-claude-bots','Websearch: Claude bots / Claude Code GitHub Action','websearch','pending','analysis/11-websearch-claude-bots.md'),
 ('ws3-openai-actions','Websearch: OpenAI agentic GitHub Actions (Codex etc.)','websearch','pending','analysis/12-websearch-openai-actions.md'),
 ('ws4-byok-custom','Websearch: BYOK custom AI agent actions + security patterns','websearch','pending','analysis/13-websearch-byok-custom-actions.md'),
 ('ws5-actions-deps','Websearch: helpful GitHub Actions dependencies/building blocks','websearch','pending','analysis/14-websearch-actions-dependencies.md'),
 ('synthesis','Synthesis of all analyses into design recommendations','synthesis','pending','analysis/00-synthesis.md');
INSERT INTO events (task_slug,event,detail) VALUES
 ('orchestrator','mission_started','Research mission for the new storytold agentic AI GitHub Action project; 12 background subagents to be spawned.');
