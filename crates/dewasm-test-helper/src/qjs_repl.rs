//! The interactive-REPL transcript case: QuickJS with no arguments, under a real pseudo-terminal.
//! There is no script argument, so `_start` sees only `argv[0]`.
//! QuickJS then drops into its interactive loop.
//! The transcript must be byte-identical to the one Wasmtime produces.
//!
//! Running with no arguments is the interactive REPL.
//! A standalone backend runs `_start` with the host process's real `argv`.
//! So the converted program spawned under the pseudo-terminal with no extra arguments is `qjs`.
//! It runs with an empty argument list.
//! That is the same shape `wasmtime run qjs.wasm` (no trailing arguments) takes.
//! The scripted session is fed with CR line endings because that is what a terminal sends on Enter.
//! The pseudo-terminal driver's ICRNL then delivers NL to the guest.
//! The guest's `stdin` reads a character device (matching Wasmtime).
//!
//! The snapshot lives at `examples/apps/snapshots/qjs_repl_interactive.transcript`.
//! It holds raw bytes, ANSI escapes included.
//! A `wasmtime_test`-conditional freshness test re-validates it against a live `wasmtime`.
//! That test is in `crates/dewasm-test-helper/tests/apps_wasmtime.rs`.

use std::path::PathBuf;
use std::time::Duration;

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_snapshot_dir};
use crate::pty::run_under_pty;

/// The scripted interactive session: three expressions and the `\q` quit command.
/// Each ends with CR, which is what a TTY sends on Enter.
/// That was verified against Wasmtime, whose guest sees the driver's CR->NL translation.
pub const QJS_REPL_SESSION: &[u8] = b"1+2\r[3,1,2].sort()\rMath.max(4,9)\r\\q\r";

/// The QuickJS REPL prompt.
/// The pseudo-terminal driver is prompt-driven off this.
/// Each scripted line is sent only after the prompt reappears.
/// So the transcript is identical no matter how long a backend takes to start.
/// See [`crate::run_under_pty`].
const QJS_PROMPT: &[u8] = b"qjs > ";

/// Hard cap on the pseudo-terminal session (fail loud).
/// Large: the interactive loop is I/O-bound line editing, not the slow batch `qjs` cases.
/// But the compiled backends may pay a one-time build inside `pty_command` first.
const PTY_TIMEOUT: Duration = Duration::from_secs(180);

pub fn qjs_repl_snapshot_path() -> PathBuf {
    apps_snapshot_dir().join("qjs_repl_interactive.transcript")
}

/// Convert the cached `qjs.wasm` to a standalone program for `lang`.
/// Drive its interactive REPL under a pseudo-terminal with [`QJS_REPL_SESSION`].
/// Return the raw transcript.
/// Shared by the conditional per-backend runner and the Wasmtime snapshot capture/freshness path.
pub fn capture_qjs_repl_transcript(lang: &dyn BackendUnderTest) -> Vec<u8> {
    let wasm = apps_cache_dir().join("qjs.wasm");
    assert!(
        wasm.exists(),
        "qjs not cached: run examples/apps/setup.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&wasm).expect("read qjs wasm");
    let source = lang.convert_app(&bytes, Mode::Standalone, "qjs");
    let cmd = lang.pty_command(&source, &[]);
    run_under_pty(cmd, QJS_REPL_SESSION, Some(QJS_PROMPT), PTY_TIMEOUT)
}

/// The per-backend runner: convert `qjs` to a standalone program for `lang`.
/// Drive its REPL under a pseudo-terminal, and require a transcript byte-identical to the snapshot.
/// The snapshot is the Wasmtime one.
/// The skip for speed lives at the macro/feature level, so this runner runs unconditionally.
/// `qjs_repl_pty_e2e!` expands its `#[test]` as `#[ignore]`d unless the `slow_test` feature is on.
pub fn run_qjs_repl_pty(lang: &dyn BackendUnderTest) {
    let snapshot = std::fs::read(qjs_repl_snapshot_path()).unwrap_or_else(|e| {
        panic!(
            "qjs repl snapshot {:?} not readable ({e}): regenerate it via the \
             wasmtime freshness test (docs/testing.md)",
            qjs_repl_snapshot_path()
        )
    });
    let got = capture_qjs_repl_transcript(lang);
    assert_transcript_eq(&got, &snapshot, lang.name());
    println!(
        "qjs interactive REPL under {}: matches the wasmtime snapshot ({} bytes)",
        lang.name(),
        got.len()
    );
}

/// Byte-compare two transcripts.
/// On mismatch, panic with an escaped, human-readable dump and the first differing offset.
/// `assert_eq!` would instead print the raw byte array for hundreds of bytes of ANSI escapes.
pub fn assert_transcript_eq(got: &[u8], want: &[u8], who: &str) {
    if got == want {
        return;
    }
    let first_diff = got
        .iter()
        .zip(want.iter())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| got.len().min(want.len()));
    panic!(
        "qjs interactive REPL under {who}: transcript differs from the wasmtime snapshot\n\
         got  ({} bytes): {}\n\
         want ({} bytes): {}\n\
         first difference at byte {first_diff}",
        got.len(),
        got.escape_ascii(),
        want.len(),
        want.escape_ascii(),
    );
}
