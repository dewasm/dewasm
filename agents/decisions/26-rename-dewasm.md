# Decision 26: Rename the Project (dewasmify → dewasm)

Status: **Accepted, 2026-07-25; fully landed 2026-07-28.**
Landed 2026-07-26: crates are `dewasm-*`, and the binary is `dewasm`.
Env vars are `DEWASM_*`, and docs are swept (superseded decision bodies keep the historical name).
The GitHub repository itself now lives at `github.com/dewasm/dewasm` (commit ae2b430).
Amends the naming note in [decision 0](0-foundation.md).

## Context

Decision 0 named the project `dewasmify` ("strips the wasm out").
In use, the name is four syllables and awkward to say.
The `-ify` suffix adds no meaning that `de-` doesn't already carry.
A rename only gets more expensive.
After 0.1 the name is in release artifacts, user scripts, and (potentially) crates.io.

Availability was checked on 2026-07-25.
No `dewasm` crate exists on crates.io (the API returns 404).
A GitHub repository search finds no project of that name (only unrelated personal accounts).

## Decision

Rename to **`dewasm`** everywhere at once, before 0.1.
That covers the repository and the binary (`dewasm`).
It covers the crate names: `dewasm-core`, `dewasm-backend`, `dewasm-backend-<lang>`, `dewasm-cli`.
It covers the env-var prefix, `DEWASM_*`.
Its variables are `DEWASM_SPEC`, `DEWASM_SPEC_ALL`, `DEWASM_APPS_ALL`, `DEWASM_UPDATE_DOCS`, `DEWASM_BASH`.
Criterion: **the last cheap moment to rename is before the first release**.
A name that will be typed for years should be short and pronounceable.
This one is verified free.

Reserving the crates.io name with an early publish is recommended at release time.
The 0.1 checklist owns that call.

## Rejected alternatives

- **Keep `dewasmify`**: avoids one churn commit.
  The friction is then paid on every future mention of the project.
- **Rename after 0.1**: strictly more expensive.
  Released artifacts, tags, and external references would then also need aliases.

## Consequences

- Positive: shorter name, free on crates.io, single flag-day commit while there are no external users.
- Negative: one wide, though purely mechanical, diff.
  Historical decision prose refers to the old name until the rename commit runs it.
  Superseded decisions keep reading coherently: it is the same project.
- The rename commit must contain no behavior changes so it reviews as a pure diff of names.
