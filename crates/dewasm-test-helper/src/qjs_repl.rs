//! The interactive-REPL transcript case (Fix 4): drive the *bare* QuickJS REPL
//! (no script argument, so `_start` sees only argv[0] and QuickJS drops into
//! its interactive loop) under a real pty and require the transcript to be
//! byte-identical to the one wasmtime produces.
//!
//! The bare no-args invocation is the interactive REPL: a standalone backend
//! runs `_start` with the host process's real argv, so spawning the converted
//! program under the pty with no extra arguments is exactly `qjs` with an empty
//! argument list — the same shape `wasmtime run qjs.wasm` (no trailing args)
//! takes. The scripted session is fed with CR line endings because that is
//! what a terminal sends on Enter; the pty driver's ICRNL then delivers NL to
//! the guest, whose stdin reads a character device (matching wasmtime).
//!
//! The golden lives at `examples/apps/golden/qjs_repl_interactive.transcript`
//! (raw bytes, ANSI escapes included) and is re-validated against a live
//! wasmtime by the `wasmtime_test`-gated freshness test in
//! `crates/dewasm-test-helper/tests/apps_wasmtime.rs`.

use std::path::PathBuf;
use std::time::Duration;

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_golden_dir};
use crate::pty::run_under_pty;

/// The scripted interactive session: three expressions and the `\q` quit
/// command, each terminated by CR (what a tty sends on Enter — verified against
/// wasmtime, whose guest sees the driver's CR->NL translation).
pub const QJS_REPL_SESSION: &[u8] = b"1+2\r[3,1,2].sort()\rMath.max(4,9)\r\\q\r";

/// The QuickJS REPL prompt. The pty driver is prompt-driven off this: each
/// scripted line is sent only after the prompt reappears, so the transcript is
/// identical no matter how long a backend takes to start (see
/// [`crate::run_under_pty`]).
const QJS_PROMPT: &[u8] = b"qjs > ";

/// Hard cap on the pty session (fail loud, ADR-15). Generous: the interactive
/// loop is I/O-bound line editing, not the heavy batch qjs cases, but the
/// compiled backends may pay a one-time build inside `pty_command` first.
const PTY_TIMEOUT: Duration = Duration::from_secs(180);

/// Path to the checked-in golden transcript.
pub fn qjs_repl_golden_path() -> PathBuf {
    apps_golden_dir().join("qjs_repl_interactive.transcript")
}

/// Convert the cached `qjs.wasm` to a standalone program for `lang` and drive
/// its interactive REPL under a pty with [`QJS_REPL_SESSION`], returning the
/// raw transcript. Shared by the gated per-backend runner and the wasmtime
/// golden capture/freshness path.
pub fn capture_qjs_repl_transcript(lang: &dyn BackendUnderTest) -> Vec<u8> {
    let wasm = apps_cache_dir().join("qjs.wasm");
    assert!(
        wasm.exists(),
        "qjs not cached — run examples/apps/fetch.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&wasm).expect("read qjs wasm");
    let source = lang.convert_app(&bytes, Mode::Standalone, "qjs");
    let cmd = lang.pty_command(&source, &[]);
    run_under_pty(cmd, QJS_REPL_SESSION, Some(QJS_PROMPT), PTY_TIMEOUT)
}

/// Capture the transcript via `lang` (the wasmtime engine-under-test) and write
/// it to the golden path, returning the bytes. Used to (re)generate the golden.
pub fn capture_qjs_repl_golden(lang: &dyn BackendUnderTest) -> Vec<u8> {
    let transcript = capture_qjs_repl_transcript(lang);
    std::fs::write(qjs_repl_golden_path(), &transcript).expect("write qjs repl golden");
    transcript
}

/// The per-backend runner: convert qjs to a standalone program for `lang`,
/// drive its REPL under a pty, and require the transcript to be
/// byte-identical to the wasmtime golden. The perf opt-out lives at the
/// macro/feature level (`qjs_repl_pty_e2e!` expands its `#[test]` as
/// `#[ignore]`d unless the `heavy_test` feature is on), so this runner runs
/// unconditionally.
pub fn run_qjs_repl_pty(lang: &dyn BackendUnderTest) {
    let golden = std::fs::read(qjs_repl_golden_path()).unwrap_or_else(|e| {
        panic!(
            "qjs repl golden {:?} not readable ({e}) — regenerate it via the \
             wasmtime freshness test (docs/testing.md)",
            qjs_repl_golden_path()
        )
    });
    let got = capture_qjs_repl_transcript(lang);
    assert_transcript_eq(&got, &golden, lang.name());
    println!(
        "qjs interactive REPL under {}: matches the wasmtime golden ({} bytes)",
        lang.name(),
        got.len()
    );
}

/// Byte-compare two transcripts, panicking with an escaped, human-readable dump
/// (and the first differing offset) rather than the raw byte-array spew
/// `assert_eq!` would produce for hundreds of bytes of ANSI escapes.
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
        "qjs interactive REPL under {who}: transcript differs from the wasmtime golden\n\
         got  ({} bytes): {}\n\
         want ({} bytes): {}\n\
         first difference at byte {first_diff}",
        got.len(),
        got.escape_ascii(),
        want.len(),
        want.escape_ascii(),
    );
}
