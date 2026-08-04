# ADR-26 — Rename the Project: dewasmify → dewasm

Status: **Accepted, 2026-07-25; fully landed 2026-07-28.** Landed 2026-07-26: crates are `dewasm-*`, the binary is `dewasm`, env vars are `DEWASM_*`, and docs are swept (superseded ADR bodies keep the historical name). The GitHub repository itself now lives at `github.com/dewasm/dewasm` (commit ae2b430). Amends the naming note in [ADR-0](0-foundation.md).

## Context

ADR-0 named the project `dewasmify` ("strips the wasm out"). In use, the name is four syllables and awkward to say; the `-ify` suffix adds no meaning that `de-` doesn't already carry. A rename only gets more expensive: after 0.1 the name is in release artifacts, user scripts, and (potentially) crates.io.

Availability was checked on 2026-07-25: no `dewasm` crate exists on crates.io (the API returns 404), and a GitHub repository search finds no project of that name (only unrelated personal accounts).

## Decision

Rename to **`dewasm`** everywhere at once, before 0.1: repository, crate names (`dewasm-core`, `dewasm-backend`, `dewasm-backend-<lang>`, `dewasm-cli`), the binary (`dewasm`), and the env-var prefix (`DEWASM_*` for `DEWASM_SPEC`, `DEWASM_SPEC_ALL`, `DEWASM_APPS_ALL`, `DEWASM_UPDATE_DOCS`, `DEWASM_BASH`). Criterion: **the last cheap moment to rename is before the first release**; a name that will be typed for years should be short and pronounceable, and this one is verified free.

Reserving the crates.io name with an early publish is recommended at release time (the 0.1 checklist owns that call).

## Rejected alternatives

- **Keep `dewasmify`** — avoids one churn commit, but the friction is paid on every future mention of the project.
- **Rename after 0.1** — strictly more expensive: released artifacts, tags, and external references would then also need aliases.

## Consequences

- Positive: shorter name, free on crates.io, single flag-day commit while there are no external users.
- Negative: one wide, though purely mechanical, diff; historical ADR prose refers to the old name until the rename commit runs it (superseded ADRs keep reading coherently — same project).
- The rename commit must contain no behavior changes so it reviews as a pure diff of names.
