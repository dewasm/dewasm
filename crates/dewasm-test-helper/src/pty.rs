//! Real-pty process driving for the interactive-REPL transcript tests (Fix 4).
//!
//! Some programs behave differently when their stdin is a terminal: the qjs REPL, for one, only enters its interactive line-editing path (banner, prompts, per-keystroke echo, result printing) when `fd_fdstat_get` on fd 0 reports a character device. Proving a converted backend byte-identical to wasmtime on that path therefore needs a genuine pty pair, not the pipes the rest of the suite uses.
//!
//! [`run_under_pty`] spawns a command on a fixed-size (80x24) pty, feeds it a scripted input, reads the whole transcript until the child exits, and returns the raw bytes (ANSI escapes and all). Two pacing strategies exist:
//!
//! * *prompt-driven* (`prompt: Some(..)`) — before each input line, wait for the prompt marker to appear in the output, then send the line. This synchronizes input with the guest's readiness, so the transcript is identical no matter how long the guest takes to start (a Ruby backend parses a ~200 MB source before qjs even runs; a fixed time delay would let the tty buffer every line into one before the guest read any of it). * *time-paced* (`prompt: None`) — write each line with a small fixed delay. Simpler, but only stable for fast-starting programs; kept as the fallback the task asked to compare against.
//!
//! WASI p1 has no `winsize`/termios surface, so the guest cannot switch the pty out of canonical mode: input stays line-buffered and the driver echoes it. With the pty size fixed, `TERM` pinned, and prompt-driven pacing, the whole transcript is deterministic across engines — that is what makes the wasmtime-vs-backend byte comparison meaningful.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// A command to run under a pty: the program path, its arguments (argv[1..]), and an optional working directory. Mirrors the `std::process::Command` surface the rest of the helper builds, but is spawned onto a pty slave.
pub struct PtyCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

/// Per-line delay for the time-paced (`prompt: None`) strategy.
const LINE_PACING: Duration = Duration::from_millis(120);

/// Spawn `cmd` on a fresh 80x24 pty, feed it `input` (see the module docs for the two pacing strategies selected by `prompt`), read the full transcript until the child exits, and return the raw bytes.
///
/// The pty size is fixed and `TERM` is pinned to a constant so the transcript does not vary with the developer's terminal. A child that does not exit — or a prompt that never appears — within `timeout` is killed and the call panics (fail loud, ADR-15) rather than hanging the suite.
pub fn run_under_pty(
    cmd: PtyCommand,
    input: &[u8],
    prompt: Option<&[u8]>,
    timeout: Duration,
) -> Vec<u8> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize::default()) // 80 cols x 24 rows
        .expect("openpty");

    let mut builder = CommandBuilder::new(&cmd.program);
    for arg in &cmd.args {
        builder.arg(arg);
    }
    if let Some(cwd) = &cmd.cwd {
        builder.cwd(cwd);
    }
    // Deterministic terminal: `CommandBuilder::new` already inherits the parent environment, so only pin the one variable a terminal type could vary the output by. wasmtime and every backend go through this same helper, so both sides see an identical TERM.
    builder.env("TERM", "xterm-256color");

    let mut child = pair.slave.spawn_command(builder).expect("spawn on pty");
    // Drop our handle to the slave so that, once the child closes its own copy on exit, the master read returns EOF instead of blocking forever.
    drop(pair.slave);

    // Reader thread: pump every chunk over a channel so the driving loop can wait on output (for the prompt) with a timeout without blocking on a read that may never return.
    let mut reader = pair.master.try_clone_reader().expect("clone pty reader");
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let reader_handle = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(chunk[..n].to_vec()).is_err() {
                        break;
                    }
                }
                // A closed pty surfaces as an EIO read error on some platforms rather than a clean EOF; treat it as end-of-transcript.
                Err(_) => break,
            }
        }
    });

    let deadline = Instant::now() + timeout;
    let mut transcript = Vec::new();
    let mut writer = pair.master.take_writer().expect("take pty writer");

    let lines = split_inclusive_newlines(input);
    let mut search_from = 0usize;
    for line in lines {
        if let Some(marker) = prompt {
            // Wait for the next prompt (strictly after the previous one) before sending, so slow-starting guests never miss buffered input.
            match pump_until_marker(&mut transcript, &rx, marker, search_from, deadline) {
                Some(pos) => search_from = pos + marker.len(),
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!(
                        "run_under_pty: prompt {:?} did not appear within {timeout:?}; \
                         captured {} bytes so far:\n{}",
                        String::from_utf8_lossy(marker),
                        transcript.len(),
                        String::from_utf8_lossy(&transcript),
                    );
                }
            }
        }
        writer.write_all(line).expect("write pty input");
        writer.flush().ok();
        if prompt.is_none() {
            std::thread::sleep(LINE_PACING);
        }
    }

    // All input sent; drain the remaining output until the child exits.
    loop {
        match child.try_wait().expect("try_wait pty child") {
            Some(_status) => break,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    drain(&mut transcript, &rx, Duration::from_millis(200));
                    panic!(
                        "run_under_pty: child {:?} did not exit within {timeout:?}; \
                         captured {} bytes so far:\n{}",
                        cmd.program,
                        transcript.len(),
                        String::from_utf8_lossy(&transcript),
                    );
                }
                // Keep pulling output while we wait so nothing is lost.
                if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(20)) {
                    transcript.extend_from_slice(&chunk);
                }
            }
        }
    }

    // Child is gone; drop the master to guarantee the reader observes EOF, then collect whatever is left in flight.
    drop(writer);
    drop(pair.master);
    drain(&mut transcript, &rx, Duration::from_millis(500));
    let _ = reader_handle.join();
    transcript
}

/// Pump output from `rx` into `transcript` until `marker` appears at or after byte `from`, returning its absolute index, or `None` on timeout / the child closing its output before the marker showed up.
fn pump_until_marker(
    transcript: &mut Vec<u8>,
    rx: &mpsc::Receiver<Vec<u8>>,
    marker: &[u8],
    from: usize,
    deadline: Instant,
) -> Option<usize> {
    loop {
        if let Some(rel) = find_subslice(&transcript[from.min(transcript.len())..], marker) {
            return Some(from + rel);
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let wait = (deadline - now).min(Duration::from_millis(50));
        match rx.recv_timeout(wait) {
            Ok(chunk) => transcript.extend_from_slice(&chunk),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // Output closed (child exited) before the marker: one last check.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return find_subslice(&transcript[from.min(transcript.len())..], marker)
                    .map(|rel| from + rel);
            }
        }
    }
}

/// Drain any bytes still queued in `rx` into `transcript`, giving up after `quiet` elapses with nothing new (used once the child has exited).
fn drain(transcript: &mut Vec<u8>, rx: &mpsc::Receiver<Vec<u8>>, quiet: Duration) {
    while let Ok(chunk) = rx.recv_timeout(quiet) {
        transcript.extend_from_slice(&chunk);
    }
}

/// First index of `needle` within `haystack`, or `None`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Split `input` into lines, keeping each line's terminating `\r` or `\n` (a bare CR is what a terminal sends on Enter). A trailing unterminated segment is yielded as its own line.
fn split_inclusive_newlines(input: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, &b) in input.iter().enumerate() {
        if b == b'\r' || b == b'\n' {
            lines.push(&input[start..=i]);
            start = i + 1;
        }
    }
    if start < input.len() {
        lines.push(&input[start..]);
    }
    lines
}
