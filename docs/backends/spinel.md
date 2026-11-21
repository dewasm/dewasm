# Spinel backend

`--target spinel`.
One Ruby source file for [Spinel](https://github.com/matz/spinel), which compiles Ruby ahead of time.
The result is a native executable.

## Output shape

The output is the [Ruby backend's](ruby.md), with one difference: how it holds an i64.

| Value | Ruby backend | Spinel backend |
| --- | --- | --- |
| i32 | `0..=2**32 - 1` | the same |
| i64 | `0..=2**64 - 1` | a signed 64-bit word, `-2**63..=2**63 - 1` |

Spinel's `Integer` is one machine word.
Under `--int-overflow=wrap`, its `+`, `-`, `*` and `<<` wrap at 64 bits.
So an i64 `add`, `sub`, `mul` or `shl` is the host's own operator, with no mask after it.
The operations that read an i64 as unsigned have their own lowering:

| Operation | Lowering |
| --- | --- |
| unsigned compare | both sides with the sign bit flipped, then a signed compare |
| `shr_u` | `>>`, then a mask that clears the copied sign bits |
| `div_u`, `rem_u`, conversions to a float | a runtime helper |

Everything else is as the Ruby backend emits it.
That covers the class shape, the control flow, the memory, and the WASI runtime.

## Requirements

A `spinel` on `PATH` (or `$DEWASM_SPINEL`), at the commit `mise.toml` states, or newer.
`mise install` builds that commit from its source.

The program must be built with `--int-overflow=wrap`.
The runtime checks this when it starts.
A build with another mode stops with a message that names the flag, and so does a run under `ruby`.

## Running it

```console
$ dewasm prog.wasm --target spinel --mode standalone -o prog.rb
$ spinel --int-overflow=wrap prog.rb -o prog
$ ./prog arg1 arg2
```

Standalone programs share one runtime interface: `argv`, `--dir` preopens, `--env` variables, exit/trap.
It is described in [`docs/standalone-interface.md`](../standalone-interface.md).

## Embedding a library artifact

A library artifact is a Ruby class, as for the Ruby backend, and Spinel compiles it with its caller.

```console
$ dewasm add.wasm --target spinel --mode library --module-name Add -o add.rb
```

An i64 argument or result crosses that boundary as a signed word.
So the i64 with every bit set is `-1` here, where the Ruby backend gives `18446744073709551615`.

## Limits

- The output does not run under `ruby`.
- At `-O 2`, Spinel's default, a recursion with no end can run forever where wasm traps.
  The C compiler turns such a function into a loop, so the stack never fills.
- A `poll_oneoff` clock timeout of `2**63` nanoseconds or more is read as a time in the past.
