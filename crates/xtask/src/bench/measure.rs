//! Process execution and the timing primitives the measurement design rests on.
//!
//! Three things happen here, and each exists to remove a specific way this comparison could lie:
//!
//! * [`run_once`] times one whole process — spawn to exit — with stdin fed and both output streams drained concurrently, under a hard timeout. The timer starts *before* `spawn`, so process startup is inside the measurement; that is deliberate, because the `<iterations> = 0` run subtracts it back out again.
//! * [`calibrate`] picks the iteration count *per runner*. The spread between the fastest and slowest runner here is around 1000x, so one fixed count either measures nothing but startup on wasmtime or takes minutes per sample on Bash. Calibration ramps from a trivial count until the compute time reaches the target, capped by the workload's own ceiling — so nothing in the suite depends on a hand-guessed per-runner divisor.
//! * [`stats`] reports the minimum *and* the median of the timed repetitions. The minimum is the least noise-contaminated estimate; publishing the median next to it is what makes the noise visible instead of hidden.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::bench::runner::Launch;

/// One process execution.
pub struct RunOutcome {
    /// Wall time from just before `spawn` to the child's exit.
    pub wall: Duration,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: Option<i32>,
    /// The child outlived the timeout and was killed; `wall` and the output are meaningless.
    pub timed_out: bool,
}

impl RunOutcome {
    /// `Ok(())` when the child exited 0 in time; otherwise a one-line reason carrying the tail of stderr, which is where every runner in this suite puts its diagnostics.
    pub fn require_success(&self, what: &str) -> Result<()> {
        if self.timed_out {
            return Err(anyhow::anyhow!("{what}: timed out"));
        }
        if self.code == Some(0) {
            return Ok(());
        }
        let tail: String = String::from_utf8_lossy(&self.stderr)
            .lines()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .join(" | ");
        Err(anyhow::anyhow!("{what}: exit {:?}: {tail}", self.code))
    }

    /// The `load_ms=<float>` line the third-party drivers print on stderr for module load/instantiate time. The last occurrence wins, so a driver that logs progress before it can be silent.
    pub fn load_ms(&self) -> Option<f64> {
        String::from_utf8_lossy(&self.stderr)
            .lines()
            .filter_map(|line| line.trim().strip_prefix("load_ms=")?.trim().parse().ok())
            .next_back()
    }
}

/// Run `launch` with `args` appended and `stdin` fed, timing the whole process.
///
/// stdout and stderr are drained on their own threads: a runner that writes more than a pipe buffer while we are blocked writing stdin would otherwise deadlock. The timeout is enforced by a watchdog that `kill -9`s the child, so a runner that turns out to be 10000x rather than 1000x slower costs one timeout instead of hanging the suite.
pub fn run_once(
    launch: &Launch,
    args: &[String],
    stdin: &[u8],
    timeout: Duration,
) -> Result<RunOutcome> {
    let mut cmd = Command::new(&launch.program);
    cmd.args(&launch.args).args(args);
    for (key, value) in &launch.env {
        cmd.env(key, value);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let start = Instant::now();
    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn {}", launch.program.display()))?;
    let pid = child.id();
    let mut child_stdin = child.stdin.take().expect("stdin was piped");
    let mut child_stdout = child.stdout.take().expect("stdout was piped");
    let mut child_stderr = child.stderr.take().expect("stderr was piped");

    let done = Arc::new(AtomicBool::new(false));
    let killed = Arc::new(AtomicBool::new(false));
    let watchdog = {
        let (done, killed) = (Arc::clone(&done), Arc::clone(&killed));
        std::thread::spawn(move || {
            let deadline = Instant::now() + timeout;
            loop {
                if done.load(Ordering::Relaxed) {
                    return;
                }
                let now = Instant::now();
                if now >= deadline {
                    killed.store(true, Ordering::Relaxed);
                    let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
                    return;
                }
                std::thread::park_timeout(deadline - now);
            }
        })
    };

    let input = stdin.to_vec();
    // Dropping the handle at the end of the closure closes the pipe, which is the EOF every stdin-driven workload here waits for.
    let writer = std::thread::spawn(move || {
        let _ = child_stdin.write_all(&input);
    });
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = child_stdout.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = child_stderr.read_to_end(&mut buf);
        buf
    });

    let status = child.wait().context("failed to wait for the child")?;
    let wall = start.elapsed();
    done.store(true, Ordering::Relaxed);
    watchdog.thread().unpark();

    let _ = writer.join();
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    let _ = watchdog.join();

    Ok(RunOutcome {
        wall,
        stdout,
        stderr,
        code: status.code(),
        timed_out: killed.load(Ordering::Relaxed),
    })
}

/// One repetition series: a warmup run (discarded) followed by `reps` timed runs. Returns the samples in seconds together with the last run's output, which is what the correctness cross-check compares.
pub fn repeat(
    launch: &Launch,
    args: &[String],
    stdin: &[u8],
    reps: usize,
    timeout: Duration,
    what: &str,
) -> Result<(Vec<f64>, RunOutcome)> {
    let warmup = run_once(launch, args, stdin, timeout)?;
    warmup.require_success(what)?;
    let mut samples = Vec::with_capacity(reps);
    let mut last = warmup;
    for _ in 0..reps {
        let outcome = run_once(launch, args, stdin, timeout)?;
        outcome.require_success(what)?;
        samples.push(outcome.wall.as_secs_f64());
        last = outcome;
    }
    Ok((samples, last))
}

/// Pick the iteration count for one (workload, runner) pair: ramp from 1 until `t(N) - t_zero` reaches `target`, never exceeding `cap`.
///
/// `t_zero` is the already-measured `<iterations> = 0` time, so the ramp reasons about compute alone and is not fooled by a runner whose startup dwarfs its work (loading the generated SQLite Ruby costs 0.7 s before anything runs). The growth factor is clamped to 256x per round so one noisy sample cannot overshoot into a multi-minute run; eight rounds is enough to cross the ~30 million iterations the fastest runner needs.
pub fn calibrate(
    mut run: impl FnMut(u64) -> Result<RunOutcome>,
    t_zero: f64,
    target: Duration,
    cap: u64,
) -> Result<u64> {
    let target = target.as_secs_f64();
    let mut iters = 1u64;
    for _ in 0..8 {
        if iters >= cap {
            return Ok(cap);
        }
        let outcome = run(iters)?;
        outcome.require_success(&format!("calibration at {iters} iterations"))?;
        let compute = outcome.wall.as_secs_f64() - t_zero;
        if compute >= target * 0.6 {
            return Ok(iters);
        }
        let factor = if compute <= 0.0 {
            256.0
        } else {
            (target / compute).clamp(2.0, 256.0)
        };
        iters = cap.min(((iters as f64) * factor) as u64).max(iters + 1);
    }
    Ok(iters.min(cap))
}

/// The minimum and the median of `samples`, in seconds. Publishing both is the point: a large gap between them means the host was noisy and the numbers should not be read closely.
pub fn stats(samples: &[f64]) -> (f64, f64) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let min = sorted.first().copied().unwrap_or(f64::NAN);
    let median = match sorted.len() {
        0 => f64::NAN,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0,
    };
    (min, median)
}

/// A short description of how two stdout captures differ, for a cross-check failure message. Byte lengths plus the first differing line say enough to recognize a numeric-semantics bug (a float printed with the wrong rounding) without dumping megabytes into the report.
pub fn describe_diff(expected: &[u8], actual: &[u8]) -> String {
    let (expected, actual) = (
        String::from_utf8_lossy(expected),
        String::from_utf8_lossy(actual),
    );
    let first = expected
        .lines()
        .zip(actual.lines())
        .find(|(a, b)| a != b)
        .map(|(a, b)| format!("wasmtime {a:?} vs runner {b:?}"))
        .unwrap_or_else(|| {
            format!(
                "same prefix, different length ({} vs {} lines)",
                expected.lines().count(),
                actual.lines().count()
            )
        });
    format!(
        "stdout mismatch ({} vs {} bytes): {first}",
        expected.len(),
        actual.len()
    )
}
