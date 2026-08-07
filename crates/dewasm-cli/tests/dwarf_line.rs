//! End-to-end coverage for `--dwarf-line` source back-mapping.
//!
//! Semantics-neutrality is the whole contract: the flag adds source-position markers (Go `//line`, Ruby comments) and changes nothing else. Each case converts the cached DWARF fixture both with and without the flag, then asserts (a) the flagged output actually carries fixture markers, (b) it renders and runs to the same stdout/exit as the plain output, and (c) stripping the marker lines from the flagged source yields the plain source byte-for-byte.
//!
//! The Go case additionally pins the two `//line` gotchas: the directive is emitted at column 1 (Go honors it nowhere else) and never with a `line 0` (which `go build` rejects).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dewasm_backend_go::find_go;
use dewasm_backend_ruby::find_ruby;

fn dewasm_bin() -> &'static str {
    env!("CARGO_BIN_EXE_dewasm")
}

/// The cached DWARF fixture; a missing cache fails loud, never skips.
fn fixture_wasm() -> PathBuf {
    let p =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/cache/dwarf-fixture.wasm");
    assert!(
        p.exists(),
        "dwarf-fixture not cached — run examples/apps/setup.sh (see docs/testing.md)"
    );
    p
}

/// add_mul(3,5)=23, sum_prefix([2,4,6,8],4)=20, so main prints their sum.
const FIXTURE_STDOUT: &str = "43\n";

fn tempdir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dewasm-dwarf-line-{}-{}-{tag}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_dewasm(args: &[&str]) -> Output {
    Command::new(dewasm_bin())
        .args(args)
        .output()
        .expect("spawn dewasm")
}

/// Convert the fixture to `target` at `out`, with `--dwarf-line` iff `dwarf`.
fn convert(target: &str, out: &Path, dwarf: bool) -> String {
    let wasm = fixture_wasm();
    let mut args = vec![
        wasm.to_str().unwrap(),
        "-t",
        target,
        "-m",
        "standalone",
        "-o",
        out.to_str().unwrap(),
    ];
    if dwarf {
        args.push("--dwarf-line");
    }
    let r = run_dewasm(&args);
    assert!(
        r.status.success(),
        "{target} convert (dwarf={dwarf}) failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    std::fs::read_to_string(out).unwrap()
}

/// `go build` a program in its own directory and run it, returning (stdout, exit code). A missing toolchain fails loud.
fn run_go(prog: &Path) -> (String, i32) {
    let go =
        find_go().expect("go toolchain not found on PATH (or $DEWASM_GO) — see docs/testing.md");
    let dir = prog.parent().unwrap();
    let bin = dir.join("prog_bin");
    let build = Command::new(&go)
        .arg("build")
        .arg("-o")
        .arg(&bin)
        .arg(prog.file_name().unwrap())
        .current_dir(dir)
        .output()
        .expect("spawn go build");
    assert!(
        build.status.success(),
        "go build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let out = Command::new(&bin)
        .current_dir(dir)
        .output()
        .expect("spawn go program");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// Run a Ruby program, returning (stdout, exit code).
fn run_ruby(prog: &Path) -> (String, i32) {
    let ruby = find_ruby().expect("ruby >= 3.4 not found on PATH — see docs/testing.md");
    let out = Command::new(ruby)
        .arg(prog)
        .current_dir(prog.parent().unwrap())
        .output()
        .expect("spawn ruby");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// Drop the source-line marker lines, so the remainder must equal the plain (no-flag) output. `is_marker` recognizes a backend's marker line.
fn strip_markers(src: &str, is_marker: impl Fn(&str) -> bool) -> String {
    src.lines()
        .filter(|l| !is_marker(l))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// A Ruby source-line marker: `# <path>:<line>` (possibly indented).
fn is_comment_marker(line: &str) -> bool {
    let t = line.trim_start();
    let Some(rest) = t.strip_prefix("# ") else {
        return false;
    };
    match rest.rsplit_once(':') {
        Some((_, num)) => !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

#[test]
fn go_dwarf_line_markers_are_neutral_and_build() {
    let dir = tempdir("go");
    // Distinct dirs so each program builds in isolation.
    let pdir = dir.join("plain");
    let ddir = dir.join("dwarf");
    std::fs::create_dir_all(&pdir).unwrap();
    std::fs::create_dir_all(&ddir).unwrap();
    let plain_path = pdir.join("main.go");
    let dwarf_path = ddir.join("main.go");

    let plain = convert("go", &plain_path, false);
    let dwarf = convert("go", &dwarf_path, true);

    // The flag must actually produce fixture markers.
    let fixture_markers: Vec<&str> = dwarf
        .lines()
        .filter(|l| l.starts_with("//line ") && l.contains("dwarf_fixture.c"))
        .collect();
    assert!(
        !fixture_markers.is_empty(),
        "expected //line directives into dwarf_fixture.c"
    );
    // Go honors `//line` only at column 1: never indented, never `line 0`.
    for l in dwarf
        .lines()
        .filter(|l| l.trim_start().starts_with("//line "))
    {
        assert!(
            l.starts_with("//line "),
            "//line directive must sit at column 1, got: {l:?}"
        );
        let after = l.rsplit(':').next().unwrap();
        assert_ne!(after, "0", "//line must never carry line 0: {l:?}");
    }

    // Neutrality: stripping every `//line` directive reproduces the plain file.
    let stripped = strip_markers(&dwarf, |l| l.starts_with("//line "));
    assert_eq!(
        stripped, plain,
        "flagged Go source must equal plain after stripping //line"
    );

    // Same runtime behavior.
    let (out_p, code_p) = run_go(&plain_path);
    let (out_d, code_d) = run_go(&dwarf_path);
    assert_eq!(out_p, FIXTURE_STDOUT);
    assert_eq!((out_p, code_p), (out_d, code_d));
}

#[test]
fn ruby_dwarf_line_markers_are_neutral_and_run() {
    let dir = tempdir("ruby");
    let plain_path = dir.join("plain.rb");
    let dwarf_path = dir.join("dwarf.rb");

    let plain = convert("ruby", &plain_path, false);
    let dwarf = convert("ruby", &dwarf_path, true);

    assert!(
        dwarf
            .lines()
            .any(|l| is_comment_marker(l) && l.contains("dwarf_fixture.c")),
        "expected a Ruby comment marker into dwarf_fixture.c"
    );

    // Neutrality: markers are the only additions.
    let stripped = strip_markers(&dwarf, is_comment_marker);
    assert_eq!(
        stripped, plain,
        "flagged Ruby source must equal plain after stripping markers"
    );

    let (out_p, code_p) = run_ruby(&plain_path);
    let (out_d, code_d) = run_ruby(&dwarf_path);
    assert_eq!(out_p, FIXTURE_STDOUT);
    assert_eq!((out_p, code_p), (out_d, code_d));
}

/// `--dwarf-line` is accepted by every target, whether it renders markers (Python, Perl) or drops them (Bash, Java): only that conversion succeeds is asserted here, marker content is the Go and Ruby cases' business.
#[test]
fn dwarf_line_flag_is_accepted_by_all_targets() {
    for target in ["bash", "python", "perl", "java"] {
        let dir = tempdir(target);
        let out = dir.join(format!("out.{target}"));
        let r = run_dewasm(&[
            fixture_wasm().to_str().unwrap(),
            "-t",
            target,
            "-m",
            "standalone",
            "-o",
            out.to_str().unwrap(),
            "--dwarf-line",
        ]);
        assert!(
            r.status.success(),
            "{target} must accept --dwarf-line: {}",
            String::from_utf8_lossy(&r.stderr)
        );
    }
}
