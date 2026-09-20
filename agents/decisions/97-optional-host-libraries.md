# Decision 97: A Host Library the Runtime May Lack Is Optional, and Its Absence Is Refused, Not Faked

Status: **Accepted, 2026-09-20.**
Landed: the Ruby WASI `path_link` unit ([`crates/dewasm-backend-ruby/units/wasi/path_link.rb`](../../crates/dewasm-backend-ruby/units/wasi/path_link.rb)) binds `linkat(2)` through Fiddle when Fiddle is there and falls back to `File.link` when it is not, answering `ENOTSUP` for the one request that fallback cannot serve faithfully.
Nothing changes under CRuby, which has Fiddle.

## Context

`path_link` without `SYMLINK_FOLLOW` must hardlink a symlink itself rather than its target.
`link(2)` does that on Linux and follows the symlink on macOS/BSD, so the unit reached for `linkat(..., 0)`, which is nofollow everywhere, through Fiddle: `require "fiddle"` at the top of the helper and `Fiddle::Function.new` under it.

Fiddle is a dynamic FFI, and a runtime need not have one.
An ahead-of-time compiled program has no way to bind a C symbol it did not link, and there the `require` does not even fail loudly: it warns and carries on, so the failure surfaces later as `uninitialized constant Function` while the guest is running (decision 96 is the same shape of problem, one layer down).
Generated output that dies at run time on a runtime the user chose is a worse answer than generated output that says what it cannot do.

## Decision

A host library the runtime may lack is optional: the unit tests for it, keeps the full-fidelity path when it is there, and takes the best faithful fallback when it is not.

What decides the fallback is whether it can still be *correct*, not whether it can still produce an answer.
`File.link` is the same call with the same result for every hardlink of a regular file, and on Linux for a symlink too, so that is the fallback.
The single case it cannot serve, a symlink source on a platform whose `link(2)` follows, is refused with `ENOTSUP` rather than quietly hardlinking the target, which would be a wrong file silently.

The test is the constant, not the `require`: a runtime that ignores an unavailable `require` still has to fail the `Fiddle::Function` lookup, so the unit rescues that (`LoadError`, `NameError`, `NoMethodError`) and memoizes `false`.

## Rejected alternatives

- **Keep the hard Fiddle dependency**: correct on CRuby and broken everywhere else, with the breakage arriving mid-run as a `NameError` from inside a syscall.
- **Fall back to `File.link` unconditionally**: it silently hardlinks the symlink's target on macOS, so a guest asking for one file gets another, and the conformance suite cannot see the difference because the call succeeds.
- **Refuse `path_link` entirely without Fiddle**: gives up every correct hardlink (all of Linux, and every regular file on macOS) to avoid one incorrect case.
- **Declare `path_link` unsupported for Ruby**: it is supported, on the runtime the project measures against; a capability declaration describes the backend, not the host library inventory of whatever runs the output.

## Consequences

- Positive: converted Ruby with a `path_link` import compiles and runs on a runtime without Fiddle, which is the whole of the WASI filesystem surface for such a runtime rather than one syscall.
- Negative: on macOS without Fiddle, hardlinking a symlink answers `ENOTSUP`, a documented gap in [`docs/backends/ruby.md`](../../docs/backends/ruby.md) rather than a silent wrong answer.
  The conformance suite runs under CRuby, so the suite does not cover the fallback.
- Carry-over: the same rule applies to any later unit that reaches for an optional host library; today `path_link` is the only one.

See also: [decision 96](96-generated-code-compiles-ahead-of-time.md) (the same concern for language constructs rather than libraries), [decision 14](14-ruby-wasi-filesystem.md) (the filesystem model this sits in), [decision 49](49-spec-silent-follow-wasmtime.md) (whose behavior the syscalls copy).
