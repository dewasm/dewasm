@AGENTS.md

<!-- The contract lives in AGENTS.md so every agent gets it; the `@AGENTS.md` import above is how Claude Code loads it (it reads CLAUDE.md, not AGENTS.md). Add only Claude-specific rules below; anything true for every agent belongs in AGENTS.md. -->

## Claude Code

- Claude Code finds the skills in [`.claude/skills/`](.claude/skills/) by itself.
  Each `SKILL.md`'s `description:` routes to it, so no catalogue is kept here.
  `dewasm-decision-author` routes to the authoring procedure for a new decision under `agents/decisions/`.
- A subagent does not inherit this contract.
  Restate what binds it in its prompt: the specification tests must pass, and `tests/spec` is read-only.
