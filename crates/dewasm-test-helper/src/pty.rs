//! Process driving on a real pseudo-terminal for the interactive-REPL transcript tests.
//!
//! Some programs behave differently when their `stdin` is a terminal.
//! The `qjs` REPL, for one, only enters its interactive line-editing path under one condition.
//! `fd_fdstat_get` on file descriptor 0 must report a character device.
//! That path covers the banner, prompts, the echo of each key press, and result printing.
//! So proving a backend byte-identical to Wasmtime there needs a genuine pseudo-terminal pair.
//! The pipes the rest of the suite uses are not enough.
//!
//! [`run_under_pty`] spawns a command on a fixed-size (80x24) pseudo-terminal.
//! It feeds the command a scripted input.
//! It reads the whole transcript until the child exits.
//! It returns the raw bytes, ANSI escapes and all.
//! Two pacing strategies exist:
//!
//! * *prompt-driven* (`prompt: Some(..)`): before each input line, wait for the prompt marker.
//!   Send the line once the marker appears in the output.
//!   This makes input wait for the guest's readiness.
//!   So the transcript is identical no matter how long the guest takes to start.
//!   A Ruby backend parses a ~200 MB source before `qjs` even runs.
//!   A fixed time delay would let the TTY buffer every line into one.
//!   That would happen before the guest read any of it.
//! * *time-paced* (`prompt: None`): write each line with a small fixed delay.
//!   Simpler, but only stable for fast-starting programs.
//!
//! WASI p1 has no `winsize`/`termios` surface.
//! So the guest cannot switch the pseudo-terminal out of canonical mode: input stays line-buffered.
//! The driver echoes it.
//! The pseudo-terminal size is fixed, `TERM` is set to a constant, and pacing is prompt-driven.
//! With those, the whole transcript is deterministic across engines.
//! That is what makes the byte comparison of Wasmtime with a backend meaningful.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// A command to run under a pseudo-terminal.
/// It holds the program path, its arguments (`argv[1..]`), and an optional working directory.
/// It mirrors the `std::process::Command` surface the rest of the helper builds.
/// It is spawned onto a pseudo-terminal slave instead.
pub struct PtyCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

/// Per-line delay for the time-paced (`prompt: None`) strategy.
const LINE_PACING: Duration = Duration::from_millis(120);

/// Spawn `cmd` on a fresh 80x24 pseudo-terminal, and feed it `input`.
/// `prompt` selects one of the two pacing strategies in the module documentation.
/// Read the full transcript until the child exits, and return the raw bytes.
///
/// The pseudo-terminal size is fixed and `TERM` is set to a constant.
/// So the transcript does not vary with the developer's terminal.
/// A child that does not exit (or a prompt that never appears) within `timeout` is killed.
/// The call then panics (fail loud) rather than hanging the suite.
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
    // Deterministic terminal: `CommandBuilder::new` already inherits the parent environment.
    // So only set the one variable a terminal type could vary the output by.
    // Wasmtime and every backend use this same helper, so both sides see an identical `TERM`.
    builder.env("TERM", "xterm-256color");

    let mut child = pair.slave.spawn_command(builder).expect("spawn on pty");
    // Drop our handle to the slave.
    // Once the child closes its own copy on exit, the master read then returns EOF.
    // It does not block forever.
    drop(pair.slave);

    // Reader thread: pump every chunk over a channel.
    // The driving loop can then wait on output (for the prompt) with a timeout.
    // It never blocks on a read that may never return.
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
                // On some platforms a closed pseudo-terminal reads as EIO, not a clean EOF.
                // Treat it as end-of-transcript.
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
            // Wait for the next prompt (strictly after the previous one) before sending.
            // So slow-starting guests never miss buffered input.
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

    // Child is gone; drop the master to guarantee the reader observes EOF.
    // Then collect whatever is left in flight.
    drop(writer);
    drop(pair.master);
    drain(&mut transcript, &rx, Duration::from_millis(500));
    let _ = reader_handle.join();
    transcript
}

/// Pump output from `rx` into `transcript` until `marker` appears at or after byte `from`.
/// Return its absolute index.
/// Return `None` on timeout, or when the child closes its output before the marker showed up.
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

/// Drain any bytes still queued in `rx` into `transcript`.
/// Give up after `quiet` elapses with nothing new (used once the child has exited).
fn drain(transcript: &mut Vec<u8>, rx: &mpsc::Receiver<Vec<u8>>, quiet: Duration) {
    while let Ok(chunk) = rx.recv_timeout(quiet) {
        transcript.extend_from_slice(&chunk);
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Split `input` into lines, keeping each line's ending `\r` or `\n`.
/// A lone CR is what a terminal sends on Enter.
/// A trailing segment with no line ending is yielded as its own line.
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
