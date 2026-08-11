# DOOM (Bash, ANSI terminal)

As far as we know, this is the first time DOOM has run in Bash. Not an emulator, not a port of the C source translated by hand: `build.sh` fetches the unmodified [jacobenget/doom.wasm](https://github.com/jacobenget/doom.wasm) v0.1.0 binary (checksum-pinned into the shared apps cache) and runs it through `dewasm --target bash --mode library` to produce `doom_gen.sh` (~19MB of generated Bash, gitignored, regenerated on every build), and `main.sh` implements the module's ten host imports (console logging, save-game I/O, the game clock, and frame delivery) plus a terminal renderer and raw-mode keyboard input — the same shape as `../ruby` and `../python`. DOOM's own game logic, renderer, and state machine are entirely the generated Bash; nothing about the game itself is reimplemented.

## Honest performance

This is an existence-proof demo, not a playable game, and the numbers say so plainly: **initGame takes about 103 seconds, and each tickGame call takes about 34 seconds** (measured on an Apple Silicon laptop, headless). DOOM's internal tic rate is 35Hz — 34,000x faster than what this frontend delivers. Rendering itself is not the bottleneck: sampling and drawing one frame into the terminal costs well under a second, discussed below. The wasm execution — Bash interpreting the shareware episode's game logic and software renderer, line by line, with no JIT and no compiled fast path — is the entire cost, and it is a large one:

| Phase | Time (reference machine) |
| --- | --- |
| Source `doom_gen.sh` | ~1s |
| `doom_init` (module instantiation + data segments) | ~25s |
| `initGame` | ~103s (~1.7 min) |
| `tickGame` (each) | ~34s |
| Render one frame into the terminal | well under 1s |

This is only feasible at all because of two prior perf changes to the Bash backend: making linear memory an associative array instead of a linked-list-backed indexed array (DOOM's `initGame` never finished in 3+ CPU-hours before that change), and inlining load/store instructions instead of calling a runtime unit per access, cutting `tickGame` from 87s to 34s on top. Both were general backend changes, not anything specific to DOOM or this frontend.

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input) — budget about two minutes before the title screen even appears, and expect roughly one rendered frame every 34 seconds after that. `./run.sh --smoke` instead runs a headless self-check: it inits the game, ticks it twice (not 60 — at 34s/tick that alone is over a minute), sanity-checks the last frame, writes it to `screenshot.ppm` (ASCII PPM, P3 — plain text, so there's no risk of a stray NUL corrupting it, and Bash has no binary-safe way to write P6 cleanly either), and exits non-zero on failure. The whole self-check takes 4-5 minutes; it prints a progress line before every phase specifically so a few minutes of silence never look like a hang.

## Single-file distribution

`./dist.sh` builds `doom.bash`: the frontend with the generated library inlined behind a provenance header — one 19MB script that runs anywhere with bash >= 5, no dewasm checkout needed. A prebuilt copy is published as a Gist: **https://gist.github.com/makenowjust/b1e9c2a585183f41a5f8f61b4bc9924c**

It is a Gist rather than a file in this repository on purpose: dewasm is MIT, but the built artifact embeds the GPL-2.0 DOOM engine (doomgeneric, via jacobenget/doom.wasm) and the shareware WAD, so it is distributed separately under the engine's terms, with the attribution and license notes in its header.

## Rendering

Same half-block trick as `../ruby` and `../python`: each terminal cell shows two vertically-stacked source pixels as `▀`, colored with 24-bit truecolor SGR (`\e[38;2;R;G;Bm` for the top pixel, `\e[48;2;R;G;Bm` for the bottom). DOOM's 640x400 framebuffer (a 2x upscale of its native 320x200) lives in `doom_mem`, the module's linear memory — a plain Bash associative array, one byte per address, read directly rather than copied out through a runtime call, so the renderer just samples it.

The sampled grid is capped at 160 columns rather than 320 (128,000 vs. 48,000 byte reads per frame; either is irrelevant next to a 34-second tick, and 160 already exceeds what most terminal fonts resolve). Unlike `../ruby`, there is no frame-to-frame diffing here (a full redraw is free at this tick rate), but a repeated SGR escape is still skipped, which shrinks output a lot on DOOM's flat-color areas.

## Controls

Terminals deliver key *presses* only, never releases. At 34 seconds a tick, a timer-based hold (`../ruby`'s approach) doesn't map onto anything meaningful, so instead: whatever keys were pressed since the last tick get a `reportKeyDown` immediately before `tickGame`, and a matching `reportKeyUp` immediately after — one tick of "held down," which is as fine-grained as input can possibly get at this frame rate.

| Key | Action |
| --- | --- |
| Arrow keys | Move / turn |
| f | Fire (Ctrl isn't deliverable through a terminal) |
| Space | Use (open doors, flip switches) |
| , / . | Strafe left / right |
| Tab | Automap |
| Enter | Menu confirm |
| Escape | Menu / pause |
| Backspace | Menu back |
| 0-9, a-z | Weapon select / text entry |
| q / Ctrl-C | Quit |

Practically: DOOM boots into a title screen and a "shareware episode" menu, both of which need a few Enter presses to get through. Since each keypress only takes effect on the *next* tick and each tick is 34 seconds, press Enter, then wait — mashing it doesn't speed anything up, and the game only sees whatever was pressed at all since the previous tick, not how many times.

Shift (run) has no terminal equivalent and isn't mapped, matching `../ruby`/`../python`.

## Why saves are stubbed

`../ruby` and `../python` back save games with real files. This frontend doesn't: `gameSaving.sizeOfSaveGame` always reports 0 (every slot looks empty), and `writeSaveGame` silently discards. At 34 seconds a tick, nobody is sitting through a save/load round-trip in this frontend, and file-backed saves would only add an untested code path to a demo whose entire point is elsewhere — the existence proof, not save-game fidelity.
