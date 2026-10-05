//! Process execution and the timing primitives the measurement design rests on.
//!
//! * [`run_once`] times one whole process, spawn to exit, under a hard timeout.
//! * [`run_build`] runs one compiler to its exit, under a hard timeout of its own.
//!   The timer starts *before* `spawn`: process start is deliberately inside.
//!   That is because the `<iterations> = 0` run subtracts it back out.
//! * [`calibrate`] raises the iteration count per runner until the compute time reaches the target.
//!   A fixed count cannot serve runners thousands of times apart.
//! * [`stats`] reports minimum *and* median, so noise is visible instead of hidden.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::bench::runner::Launch;

/// A cell that went past a time limit.
/// The record reports it as such, never as a failure: the limit only bounds what a run can cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeLimit {
    Build { limit: Duration },
    Run,
}

impl std::fmt::Display for TimeLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeLimit::Build { .. } => f.write_str("the build went past its time limit"),
            TimeLimit::Run => f.write_str("the run went past its time limit"),
        }
    }
}

impl std::error::Error for TimeLimit {}

/// The time left until `deadline`, zero once it has passed.
pub fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

pub struct RunOutcome {
    /// Wall time from just before `spawn` to the child's exit.
    pub wall: Duration,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: Option<i32>,
    /// The child ran past the timeout and was killed; `wall` and the output mean nothing.
    pub timed_out: bool,
}

impl RunOutcome {
    /// `Ok(())` when the child exited 0 in time.
    /// A child killed at the time limit gives a [`TimeLimit::Run`].
    /// Otherwise a one-line reason carrying the tail of `stderr`.
    /// Every runner in this suite puts its diagnostics there.
    pub fn require_success(&self, what: &str) -> Result<()> {
        if self.timed_out {
            return Err(anyhow::Error::new(TimeLimit::Run).context(format!("{what}: timed out")));
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

    /// The `load_ms=<float>` line the third-party drivers print on `stderr`.
    /// It carries the module load/instantiate time.
    /// The last occurrence wins, so a driver that logs progress before it can be silent.
    pub fn load_ms(&self) -> Option<f64> {
        String::from_utf8_lossy(&self.stderr)
            .lines()
            .filter_map(|line| line.trim().strip_prefix("load_ms=")?.trim().parse().ok())
            .next_back()
    }
}

/// Run `launch` with `args` appended and `stdin` fed, timing the whole process.
///
/// `stdout` and `stderr` are drained on their own threads.
/// Otherwise a runner could deadlock by writing more than a pipe buffer while we write `stdin`.
/// A [`Watchdog`] applies the timeout: it kills the child and every process below it.
/// So a runner that turns out 10000x rather than 1000x slower costs one timeout, not a hung suite.
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

    let watchdog = Watchdog::arm(pid, timeout);

    let input = stdin.to_vec();
    // Dropping the handle at the end of the closure closes the pipe.
    // Every workload here that reads `stdin` waits for that EOF.
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
    let timed_out = watchdog.disarm();

    let _ = writer.join();
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();

    Ok(RunOutcome {
        wall,
        stdout,
        stderr,
        code: status.code(),
        timed_out,
    })
}

/// Run a compiler to its exit under `limit`, failing with its output when it does not succeed.
///
/// At the limit the compiler is killed with every process below it.
/// So a child the compiler started, such as a linker, ends with it.
pub fn run_build(what: &str, command: &mut Command, limit: Duration) -> Result<()> {
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {what}"))?;
    let watchdog = Watchdog::arm(child.id(), limit);
    let out = child
        .wait_with_output()
        .with_context(|| format!("wait for {what}"))?;
    if watchdog.disarm() {
        return Err(
            anyhow::Error::new(TimeLimit::Build { limit }).context(format!(
                "{what} timed out after {} s; $DEWASM_BUILD_TIMEOUT sets the limit",
                limit.as_secs()
            )),
        );
    }
    if !out.status.success() {
        bail!(
            "{what} failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// A thread that kills the process tree under `root` once `limit` has passed.
/// [`Watchdog::disarm`] stops it before that.
struct Watchdog {
    done: Arc<AtomicBool>,
    killed: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl Watchdog {
    fn arm(root: u32, limit: Duration) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let killed = Arc::new(AtomicBool::new(false));
        let thread = {
            let (done, killed) = (Arc::clone(&done), Arc::clone(&killed));
            std::thread::spawn(move || {
                let deadline = Instant::now() + limit;
                loop {
                    if done.load(Ordering::Relaxed) {
                        return;
                    }
                    let now = Instant::now();
                    if now >= deadline {
                        killed.store(true, Ordering::Relaxed);
                        kill_tree(root);
                        return;
                    }
                    std::thread::park_timeout(deadline - now);
                }
            })
        };
        Watchdog {
            done,
            killed,
            thread,
        }
    }

    /// Stop the watch; `true` when the tree was killed at the limit.
    fn disarm(self) -> bool {
        self.done.store(true, Ordering::Relaxed);
        self.thread.thread().unpark();
        let _ = self.thread.join();
        self.killed.load(Ordering::Relaxed)
    }
}

/// `kill -9` the process `root` and every process below it.
///
/// Killing `root` alone is not enough: a child it started keeps the output pipes open.
/// The reader then waits for that child, however long it runs.
/// Each process is stopped before its children are listed, so it cannot start one more unseen.
/// The processes stay in the caller's process group, so an interrupt at the terminal reaches them.
fn kill_tree(root: u32) {
    let signal = |name: &str, pids: &[u32]| {
        let pids = pids.iter().map(u32::to_string);
        let _ = Command::new("kill").arg(name).args(pids).output();
    };
    let mut tree = vec![root];
    let mut next = 0;
    while next < tree.len() {
        let parent = tree[next];
        next += 1;
        signal("-STOP", &[parent]);
        if let Ok(out) = Command::new("pgrep")
            .args(["-P", &parent.to_string()])
            .output()
        {
            let children = String::from_utf8_lossy(&out.stdout);
            tree.extend(
                children
                    .lines()
                    .filter_map(|line| line.trim().parse::<u32>().ok()),
            );
        }
    }
    signal("-KILL", &tree);
}

/// App sampling: one sample is the mean of `k` back-to-back executions.
/// `k` is chosen from the warm-up run's wall time so a sample lasts roughly `target`.
/// This is the iteration calibration applied at the process level.
/// It is needed because an app has no `<iterations>` to scale.
/// A run slower than the target keeps `k = 1`.
/// Returns `(k, samples, last outcome)`.
/// The measured quantity is still one whole execution; batching only steadies it.
pub fn repeat_app(
    launch: &Launch,
    args: &[String],
    stdin: &[u8],
    reps: usize,
    target: Duration,
    deadline: Instant,
    what: &str,
) -> Result<(u64, Vec<f64>, RunOutcome)> {
    let warmup = run_once(launch, args, stdin, remaining(deadline))?;
    warmup.require_success(what)?;
    let wall = warmup.wall.as_secs_f64().max(1e-6);
    let k = ((target.as_secs_f64() / wall).ceil() as u64).clamp(1, 64);
    let mut samples = Vec::with_capacity(reps);
    let mut last = warmup;
    for _ in 0..reps {
        let mut sum = 0.0;
        for _ in 0..k {
            let outcome = run_once(launch, args, stdin, remaining(deadline))?;
            outcome.require_success(what)?;
            sum += outcome.wall.as_secs_f64();
            last = outcome;
        }
        samples.push(sum / k as f64);
    }
    Ok((k, samples, last))
}

/// One series of repeated runs: a warm-up run (not counted) followed by `reps` timed runs.
/// Returns the samples in seconds together with the last run's output.
/// The correctness cross-check compares that output.
pub fn repeat(
    launch: &Launch,
    args: &[String],
    stdin: &[u8],
    reps: usize,
    deadline: Instant,
    what: &str,
) -> Result<(Vec<f64>, RunOutcome)> {
    let warmup = run_once(launch, args, stdin, remaining(deadline))?;
    warmup.require_success(what)?;
    let mut samples = Vec::with_capacity(reps);
    let mut last = warmup;
    for _ in 0..reps {
        let outcome = run_once(launch, args, stdin, remaining(deadline))?;
        outcome.require_success(what)?;
        samples.push(outcome.wall.as_secs_f64());
        last = outcome;
    }
    Ok((samples, last))
}

/// Pick the iteration count for one (workload, runner) pair.
/// Grow the count from 1 until `t(N) - t_zero` reaches `target`, never exceeding `cap`.
///
/// `t_zero` is the already-measured `<iterations> = 0` time.
/// So the calibration reasons about compute alone.
/// A runner whose start costs far more than its work then cannot fool it.
/// The growth factor is clamped to 256x per round.
/// So one noisy sample cannot jump to a multi-minute run.
/// Eight rounds are enough to cross the ~30 million iterations the fastest runner needs.
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

/// The minimum and the median of `samples`, in seconds.
/// Publishing both is the point.
/// A large gap between them means the host was noisy, and the numbers should not be read closely.
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

/// A short description of how two `stdout` captures differ, for a cross-check failure message.
/// Byte lengths plus the first differing line say enough to recognize a numeric-semantics bug.
/// An example is a float printed with the wrong rounding.
/// They do so without dumping megabytes into the report.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn a_build_that_exits_zero_in_time_succeeds() {
        run_build("true", &mut shell("true"), Duration::from_secs(30)).unwrap();
    }

    #[test]
    fn a_build_that_fails_reports_its_output() {
        let err = run_build(
            "probe",
            &mut shell("echo to-stdout; echo to-stderr >&2; exit 3"),
            Duration::from_secs(30),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("probe failed"), "{err}");
        assert!(
            err.contains("to-stdout") && err.contains("to-stderr"),
            "{err}"
        );
    }

    #[test]
    fn a_build_past_its_limit_is_a_time_limit() {
        let limit = Duration::ZERO;
        let err = run_build("probe", &mut shell("sleep 60"), limit).unwrap_err();
        assert_eq!(
            err.downcast_ref::<TimeLimit>(),
            Some(&TimeLimit::Build { limit })
        );
        assert!(
            err.to_string().contains("probe timed out after 0 s"),
            "{err}"
        );
    }

    #[test]
    fn a_run_past_its_limit_is_a_time_limit() {
        let launch = Launch {
            program: "sleep".into(),
            args: vec!["60".to_string()],
            env: Vec::new(),
        };
        let outcome = run_once(&launch, &[], b"", Duration::ZERO).unwrap();
        let err = outcome.require_success("the timed run").unwrap_err();
        assert_eq!(err.downcast_ref::<TimeLimit>(), Some(&TimeLimit::Run));
    }

    #[test]
    fn killing_a_tree_ends_the_children_the_root_started() {
        // The shell prints a line once its child exists, and then waits for that child.
        let mut child = shell("sleep 60 & echo started; wait")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        std::io::BufRead::read_line(&mut stdout, &mut line).unwrap();
        assert_eq!(line, "started\n");

        let killed_at = Instant::now();
        kill_tree(child.id());

        // The pipe reaches its end only when every process that held it is gone.
        // A child left alive would hold it for the rest of its 60 s.
        let mut rest = String::new();
        stdout.read_to_string(&mut rest).unwrap();
        child.wait().unwrap();
        assert!(killed_at.elapsed() < Duration::from_secs(30));
    }

    #[test]
    fn a_passed_deadline_leaves_no_time() {
        assert_eq!(remaining(Instant::now()), Duration::ZERO);
    }
}
