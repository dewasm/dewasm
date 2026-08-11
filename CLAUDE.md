@AGENTS.md

<!-- The contract lives in AGENTS.md so every agent gets it; the `@AGENTS.md` import above is how Claude Code loads it (it reads CLAUDE.md, not AGENTS.md). Add only Claude-specific rules below — anything true for every agent belongs in AGENTS.md. -->

## Claude Code

- Skills are auto-discovered from [`.claude/skills/`](.claude/skills/); each `SKILL.md`'s `description:` routes to it, so no catalogue is kept here. `dewasm-decision-author` routes to the authoring procedure for a new decision under `agents/decisions/`.
- A subagent does not inherit this contract; restate what binds it (the spec harness must pass, the `tests/spec` read-only rule) in its prompt.
