# DOOM (Bash, ANSI terminal)

As far as we know, this is the first time DOOM has run in Bash.
It is not an emulator, and not a port of the C source translated by hand.
`build.sh` fetches the unmodified [`jacobenget/doom.wasm`](https://github.com/jacobenget/doom.wasm) v0.1.0 binary.
The binary is checked against a fixed checksum and stored in the shared apps cache.
`dewasm --target bash --mode library` converts it to `doom_gen.sh`, ~19MB of generated Bash.
Git ignores that file, and every build regenerates it.
`main.sh` implements the module's ten host imports.
They cover console logging, save-game I/O, the game clock, and frame delivery.
`main.sh` also has a terminal renderer and raw-mode key input.
This is the same shape as `../ruby` and `../python`.
DOOM's own game logic, renderer, and state machine are entirely the generated Bash.
Nothing about the game itself is reimplemented.

## Honest performance

This is an existence-proof example, not a playable game, and the numbers say so plainly.
**`initGame` takes about 103 seconds, and each `tickGame` call takes about 34 seconds.**
Both were measured on an Apple Silicon laptop, headless.
DOOM's internal tic rate is 35Hz, 34,000x faster than what this frontend delivers.
Rendering itself does not limit the speed.
That is because sampling and drawing one frame into the terminal costs well under a second.
That cost is discussed below.
The wasm execution is Bash interpreting the shareware episode's game logic and software renderer.
It runs line by line, with no JIT and no compiled fast path.
That execution is the entire cost, and it is a large one:

| Phase | Time (reference machine) |
| --- | --- |
| Source `doom_gen.sh` | ~1s |
| `doom_init` (module instantiation + data segments) | ~25s |
| `initGame` | ~103s (~1.7 minutes) |
| `tickGame` (each) | ~34s |
| Render one frame into the terminal | well under 1s |

This is only possible at all because of two prior performance changes to the Bash backend:

- Linear memory became an associative array instead of a linked-list-backed indexed array.
  DOOM's `initGame` never finished in 3+ CPU-hours before that change.
- Load and store instructions are inlined instead of calling a runtime unit per access.
  That cut `tickGame` from 87s to 34s on top.

Both were general backend changes, not anything specific to DOOM or this frontend.

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input).
Budget about two minutes before the title screen even appears.
Expect roughly one rendered frame every 34 seconds after that.
`./run.sh --smoke` instead runs a headless self-check.
It initializes the game and ticks it twice (not 60: at 34s/tick that alone is over a minute).
It sanity-checks the last frame, writes it to `screenshot.ppm`, and exits non-zero on failure.
The file is ASCII PPM (P3), plain text, so there's no risk of an unexpected NUL byte damaging it.
Bash also has no binary-safe way to write P6 cleanly.
The whole self-check takes 4-5 minutes.
It prints a progress line before every phase, so a few minutes of silence never look like a hang.

## Single-file distribution

`./dist.sh` builds `doom.bash`.
It is the frontend with the generated library inlined, behind a header that states its source.
That makes one 19MB script that runs anywhere with Bash >= 5, with no dewasm repository needed.
A prebuilt copy is published as a **[Gist](https://gist.github.com/makenowjust/b1e9c2a585183f41a5f8f61b4bc9924c)**.

It is a Gist rather than a file in this repository on purpose.
That is because dewasm is MIT, but the artifact embeds the GPL-2.0 DOOM engine and shareware WAD.
The engine is `doomgeneric`, via `jacobenget/doom.wasm`.
The artifact is therefore distributed separately under the engine's terms.
Its header carries the attribution and license notes.

## Rendering

Same half-block trick as `../ruby` and `../python`.
Each terminal cell shows two vertically-stacked source pixels as `▀`.
The cell is colored with 24-bit color SGR.
`\e[38;2;R;G;Bm` sets the top pixel, and `\e[48;2;R;G;Bm` the bottom.
DOOM's 640x400 framebuffer is its native 320x200, scaled by 2.
It lives in `doom_mem`, the module's linear memory.
That memory is a plain Bash associative array, one byte per address.
It is read directly rather than copied out through a runtime call, so the renderer just samples it.

The sampled grid is capped at 160 columns rather than 320 (128,000 vs. 48,000 byte reads per frame).
Either count is irrelevant next to a 34-second tick.
160 columns already exceed what most terminal fonts can show.
Unlike `../ruby`, there is no frame-to-frame diffing, since a full redraw is free at this tick rate.
A repeated SGR escape is still skipped, which reduces output a lot on DOOM's flat-color areas.

## Controls

Terminals deliver key *presses* only, never releases.
At 34 seconds a tick, a timer-based hold (`../ruby`'s approach) doesn't map onto anything meaningful.
So instead, the keys pressed since the last tick get a `reportKeyDown` immediately before `tickGame`.
A matching `reportKeyUp` follows immediately after, one tick of "held down."
That is as fine-grained as input can possibly get at this frame rate.

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
| `0-9`, `a-z` | Weapon select / text entry |
| q / Ctrl-C | Exit |

Practically: DOOM boots into a title screen and a "shareware episode" menu.
Both need a few Enter presses to get through.
Each key press only takes effect on the *next* tick, and each tick is 34 seconds.
So press Enter, then wait.
Pressing it many times doesn't speed anything up.
The game only sees whatever was pressed at all since the previous tick, not how many times.

Shift (run) has no terminal equivalent and isn't mapped, matching `../ruby`/`../python`.

## Why saves are stubbed

`../ruby` and `../python` back save games with real files.
This frontend doesn't.
`gameSaving.sizeOfSaveGame` always reports 0, so every slot looks empty.
`writeSaveGame` silently drops the data.
At 34 seconds a tick, nobody is sitting through a save/load round-trip in this frontend.
File-backed saves would only add an untested code path to an example whose point is elsewhere.
That point is the existence proof, not working save games.
