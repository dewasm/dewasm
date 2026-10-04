# Decision 101: A Tool Version Is Stated Once, in a `mise` File

Status: **Accepted, 2026-10-04.**
Implemented in these places (issue #366):
- `mise.toml` and `mise.lock`, for the tools the tests need;
- `mise.bench.toml` and `mise.bench.lock`, for the tools only the speed suite needs;
- `.github/actions/mise` and `.github/workflows/ci.yml`, which install from those files;
- `docs/testing.md` and `agents/measurement-records.md`.

It replaces the install policy of [decision 92](92-wasi-sdk-c-toolchain.md), and nothing else of that decision.

## Context

Decision 92 left the install of `wasi-sdk` to each developer.
The repository fixed a version only where it installed a tool itself, which was CI.
Its carry-over said that no tool checks the local version against the one CI installs.
By the time of this decision, the versions had moved apart for more than `wasi-sdk`:

| Tool | CI | Local development |
| --- | --- | --- |
| Binaryen | 132, the upstream release | 133, a Homebrew build |
| Ruby | 3.4 | 4.0 |
| Python | 3.13 | 3.14 |
| Java | 21 | 25 |

`ci.yml` held four installs written by hand, each with a URL, and two of them twice.
Four "keep in sync" comments were the only link to local development.

Two engines of the speed suite were builds from source ([decision 93](93-engine-runners-in-speed-suite.md)).
Neither build is needed any more.
The prebuilt CPython 3.14 turns its JIT on under `PYTHON_JIT=1`.
JRuby 10.1.2.0 carries the `IO::Buffer` fix the runner probes for.

## Decision

**A tool version is stated in one file, and both a developer's install and CI's install read it.**
**A place that only describes the version names that file instead of repeating the number.**
Such a place is a comment, a document, or an error message.
Concretely:

- `mise.toml` states the version of each tool the tests need.
  `mise.lock` holds the URL and the checksum of each fetched file, per platform.
  `mise install` installs them, and CI runs the same command through `.github/actions/mise`.
- The `[env]` table of `mise.toml` sets `WASI_SDK_PATH`.
  The build scripts still read only that variable, as decision 92 states.
- `mise` is the path the documents describe, and it is not required.
  No script and no test calls it.
  A tool on PATH and a `WASI_SDK_PATH` set by hand work as before.
- The interpreters are in the file too, so CI tests the version a developer runs.
  Ruby's floor was the version CI tested, so it moves with the file: it is now 4.0.
- Ruby is built from source, through the `ruby.compile` setting.
  The prebuilt Ruby sets `PKG_CONFIG_PATH` in every process, which a guest then sees in `environ`.
  It also has no ZJIT.
- A CI job names the tools it uses, and installs only those.
- `mise.bench.toml` states the tools only the speed suite needs, and `MISE_ENV=bench` adds it.
  Decision 93 stands: an engine is host-provided, taken from a variable and then from PATH.
  The file is one way to put an engine on PATH.
- A tool with no prebuilt release to fetch stays outside both files.
  Those are `monoruby`, Spinel, `bison`, and `flex`.
  Bash and Perl come from the host, and Rust stays with `rustup` and `rust-toolchain.toml`.

## Rejected alternatives

- **Keep the installs written by hand, and add a check that compares the versions.**
  The check needs a second list of versions to compare against, so each version is still stated twice.
  It also does nothing for the local install.
- **Have `setup.sh` fetch the tools.**
  Decision 92 rejected this because it dictates how each developer manages toolchains.
  That reason holds: a developer can ignore a `mise` file, and cannot ignore a script that installs.
- **Keep the interpreters out of the file.**
  A backend page states a floor, so any newer interpreter is valid, and a fixed one narrows the range.
  But CI and local development then test different versions by accident, not by design.
  The JIT-enabled CPython was also the most costly install, and it ends only with Python in the file.
- **Test Ruby 3.4 in CI and 4.0 locally.**
  It keeps the floor, and it keeps a second Ruby version in `ci.yml`.
  The project chose to raise the floor instead.
- **Take the prebuilt Ruby, and list the three `environ` trials it breaks as Linux failures.**
  macOS already lists them, because CoreFoundation adds a variable of its own there.
  So the count-exact `environ` trials of the Ruby backend would then run on no host.
- **State the engines of the speed suite in `mise.toml`.**
  Every `mise install` would then fetch JRuby and GraalPy, about 180 MB the tests never run.
- **Fall back to `mise where wasi-sdk` in the build scripts.**
  It would work in a shell where `mise` is not active.
  But the scripts would then know two ways to find the SDK, and one of them names a tool manager.
  `mise exec --` in front of the command reaches the same result.
- **Let CI cache the installed tools.**
  The cache would compete with the build cache for the 10 GB budget.
  `mise.lock` verifies each fetched tool, so CI fetches it again on every job.

## Consequences

- Positive: each version has one place, and a checksum verifies each fetched tool on every host.
  A new machine needs `mise install` and the set-up scripts.
  CI and local development run the same release of each tool, so their stamps agree.
  The JIT-enabled CPython and JRuby need no build from source.
- Negative: `[env]` applies only where `mise` is active.
  A shell without it needs `mise exec --` in front of each command.
  Ruby 3.4 is no longer tested.
  CI fetches each tool on every job, and builds Ruby in the two jobs that run it.
- Carry-over: a developer without `mise` can still run another version, and nothing checks that.

See also:
- [decision 9](9-example-apps-from-registry.md) (the policy on fixed versions);
- [decision 92](92-wasi-sdk-c-toolchain.md) (the install policy this replaces);
- [decision 93](93-engine-runners-in-speed-suite.md) (the engines stay host-provided).
