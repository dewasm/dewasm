# Decision 97: A Host Library the Runtime May Lack Is Optional; Its Absence Is Refused, Not Stubbed

Status: **Accepted, 2026-09-20.**
Landed: the Ruby WASI `path_link` unit ([`crates/dewasm-backend-ruby/units/wasi/path_link.rb`](../../crates/dewasm-backend-ruby/units/wasi/path_link.rb)).
It binds `linkat(2)` through Fiddle when Fiddle is there, and falls back to `File.link` when it is not.
It answers `ENOTSUP` for the one request that fallback cannot serve faithfully.
Nothing changes under CRuby, which has Fiddle.

## Context

`path_link` without `SYMLINK_FOLLOW` must hard-link a symbolic link itself rather than its target.
`link(2)` does that on Linux, but follows the symbolic link on macOS/BSD.
So the unit reached through Fiddle for `linkat(..., 0)`, which follows no link on any platform.
It did so with `require "fiddle"` at the top of the helper and `Fiddle::Function.new` under it.

Fiddle is a dynamic FFI, and a runtime need not have one.
An ahead-of-time compiled program has no way to bind a C symbol it did not link.
There the `require` does not even fail loudly: it warns and carries on.
So the failure surfaces later, as `uninitialized constant Function` while the guest is running.
Decision 96 is the same shape of problem, one layer down.
Generated output can die at run time on a runtime the user chose.
That is a worse answer than generated output that says what it cannot do.

## Decision

A host library the runtime may lack is optional.
The unit tests for it, and keeps the path with the full behavior when it is there.
When it is not there, the unit takes the best fallback that stays correct.

What decides the fallback is whether it can still be *correct*.
Whether it can still produce an answer does not decide it.
`File.link` is the same call with the same result for every hard link to a regular file.
On Linux it is the same for a symbolic link too, so that is the fallback.
It cannot serve a single case: a symbolic link source on a platform whose `link(2)` follows.
That case is refused with `ENOTSUP` rather than quietly making a hard link to the target.
A hard link to the target would silently link a wrong file.

The test is the constant, not the `require`.
A runtime that ignores an unavailable `require` still has to fail the `Fiddle::Function` lookup.
So the unit rescues that failure (`LoadError`, `NameError`, `NoMethodError`) and caches `false`.

## Rejected alternatives

- **Keep the hard Fiddle dependency.**
  It is correct on CRuby and broken everywhere else.
  The failure arrives mid-run as a `NameError` from inside a system call.
- **Fall back to `File.link` unconditionally.**
  On macOS it silently hard-links the symbolic link's target.
  A guest asking for one file then gets another.
  The conformance suite cannot see the difference, because the call succeeds.
- **Refuse `path_link` entirely without Fiddle.**
  It gives up every correct hard link to avoid one wrong case.
  Those are all of Linux, and every regular file on macOS.
- **Declare `path_link` unsupported for Ruby.**
  It is supported, on the runtime the project measures against.
  A capability declaration describes the backend.
  It does not describe the host libraries of whatever runs the output.

## Consequences

- Positive: converted Ruby with a `path_link` import compiles and runs on a runtime without Fiddle.
  For such a runtime, that is the whole of the WASI file system surface rather than one system call.
- Negative: on macOS without Fiddle, a hard link to a symbolic link answers `ENOTSUP`.
  That is a documented gap in [`docs/backends/ruby.md`](../../docs/backends/ruby.md) rather than a silent wrong answer.
  The conformance suite runs under CRuby, so the suite does not cover the fallback.
- Carry-over: the same rule applies to any later unit that reaches for an optional host library.
  Today `path_link` is the only one.

See also:
- [decision 96](96-generated-code-compiles-ahead-of-time.md) (the same concern for language constructs rather than libraries);
- [decision 14](14-ruby-wasi-file-system.md) (the file system model this sits in);
- [decision 49](49-spec-silent-follow-wasmtime.md) (whose behavior the system calls copy).
