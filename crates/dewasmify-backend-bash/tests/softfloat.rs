//! Softfloat oracle (ADR-13): drives the bash float units with edge and
//! seeded-random vectors and compares every result bit-for-bit against
//! Rust's host IEEE-754 arithmetic adjusted to wasm semantics (canonical
//! NaN results, wasm min/max, the Ruby backend's trunc trap table). The
//! spec harness remains the bar (ADR-3); this is the fast development
//! net that pinpoints the exact op and operands on a regression.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

const QNAN64: i64 = 0x7ff8000000000000u64 as i64;

/// A bash >= 5 interpreter, if one is installed (macOS system bash is 3.2).
fn bash5() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("DEWASMIFY_BASH") {
        candidates.push(PathBuf::from(env));
    }
    candidates.push(PathBuf::from("bash"));
    candidates.push(PathBuf::from("/opt/homebrew/bin/bash"));
    candidates.push(PathBuf::from("/usr/local/bin/bash"));
    for candidate in candidates {
        let Ok(out) = Command::new(&candidate)
            .args(["-c", "echo ${BASH_VERSINFO[0]}"])
            .output()
        else {
            continue;
        };
        if String::from_utf8_lossy(&out.stdout).trim().parse::<u32>().unwrap_or(0) >= 5 {
            return Some(candidate);
        }
    }
    None
}

/// Deterministic vector source (constants from PCG's default stream).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

fn f64_of(bits: i64) -> f64 {
    f64::from_bits(bits as u64)
}

/// wasm arithmetic results canonicalize NaNs (our softfloat policy).
fn canon64(x: f64) -> String {
    if x.is_nan() {
        QNAN64.to_string()
    } else {
        (x.to_bits() as i64).to_string()
    }
}

const F64_EDGES: &[u64] = &[
    0x0000000000000000, // +0
    0x8000000000000000, // -0
    0x0000000000000001, // min subnormal
    0x8000000000000001,
    0x000fffffffffffff, // max subnormal
    0x800fffffffffffff,
    0x0010000000000000, // min normal
    0x8010000000000000,
    0x001fffffffffffff,
    0x0020000000000000,
    0x7fefffffffffffff, // max finite
    0xffefffffffffffff,
    0x3ff0000000000000, // 1.0
    0xbff0000000000000,
    0x3ff0000000000001, // 1.0 + ulp
    0x3fefffffffffffff, // 1.0 - ulp/2
    0x3ff8000000000000, // 1.5
    0xbff8000000000000,
    0x4000000000000000, // 2.0
    0x3fe0000000000000, // 0.5
    0x4330000000000000, // 2^52
    0x4340000000000000, // 2^53
    0x43d0000000000000, // 2^62
    0x43e0000000000000, // 2^63
    0x47e0000000000000, // near f32 overflow territory (2^127)
    0x7ff0000000000000, // +inf
    0xfff0000000000000, // -inf
    0x7ff8000000000000, // canonical NaN
    0x7ff0000000000001, // signaling NaN
    0xfff800000000dead, // payload NaN
];

struct Cases {
    input: String,
    expected: Vec<String>,
    units: BTreeSet<String>,
}

impl Cases {
    fn new() -> Cases {
        Cases { input: String::new(), expected: Vec::new(), units: BTreeSet::new() }
    }

    fn push(&mut self, op: &str, a: i64, b: i64, expect: String) {
        self.units.insert(format!("rt/{op}"));
        let _ = writeln!(self.input, "{op} {a} {b}");
        self.expected.push(expect);
    }
}

/// Random f64 pattern pairs whose exponents are close enough to exercise
/// alignment, cancellation, and gradual underflow.
fn correlated_pair(rng: &mut Lcg) -> (i64, i64) {
    let a = rng.next() as i64;
    let ea = ((a >> 52) & 0x7ff).clamp(1, 0x7fe);
    let delta = (rng.next() % 121) as i64 - 60;
    let eb = (ea + delta).clamp(0, 0x7fe);
    let b = ((rng.next() as i64) & !(0x7ffi64 << 52)) | (eb << 52);
    (a, b)
}

fn f64_arith_cases(cases: &mut Cases) {
    let mut pairs: Vec<(i64, i64)> = Vec::new();
    for &a in F64_EDGES {
        for &b in F64_EDGES {
            pairs.push((a as i64, b as i64));
        }
    }
    let mut rng = Lcg(0x5eed_0001);
    for _ in 0..1200 {
        pairs.push((rng.next() as i64, rng.next() as i64));
    }
    for _ in 0..600 {
        pairs.push(correlated_pair(&mut rng));
    }
    for &(a, b) in &pairs {
        let (x, y) = (f64_of(a), f64_of(b));
        cases.push("f64_add", a, b, canon64(x + y));
        cases.push("f64_sub", a, b, canon64(x - y));
        cases.push("f64_mul", a, b, canon64(x * y));
        cases.push("f64_div", a, b, canon64(x / y));
    }
    for &(a, _) in &pairs {
        cases.push("f64_sqrt", a, 0, canon64(f64_of(a).sqrt()));
    }
}

fn run_cases(cases: &Cases) {
    let Some(bash) = bash5() else {
        eprintln!("bash >= 5 not found; skipping softfloat oracle");
        return;
    };
    let runtime = dewasmify_backend_bash::shared_runtime(&cases.units)
        .expect("bundle float units");
    let script = format!(
        "{runtime}\nwhile IFS=' ' read -r __op __a __b; do\n  \"rt_$__op\" \"$__a\" \"$__b\"\n  __st=$?\n  if (( __st == 134 )); then\n    echo \"T $TRAP_MSG\"\n  else\n    echo \"$(( R0 ))\"\n  fi\ndone < \"$1\"\n"
    );
    // Tests run in parallel; keep scratch files distinct per invocation.
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir();
    let tag = format!("{}-{seq}", std::process::id());
    let script_path = dir.join(format!("dewasmify-softfloat-driver-{tag}.sh"));
    let input_path = dir.join(format!("dewasmify-softfloat-input-{tag}.txt"));
    std::fs::write(&script_path, &script).unwrap();
    std::fs::write(&input_path, &cases.input).unwrap();

    let output = Command::new(&bash)
        .arg(&script_path)
        .arg(&input_path)
        .output()
        .expect("run bash");
    assert!(
        output.status.success(),
        "driver failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let got: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        got.len(),
        cases.expected.len(),
        "driver produced {} results for {} cases",
        got.len(),
        cases.expected.len()
    );

    let inputs: Vec<&str> = cases.input.lines().collect();
    let mut mismatches = Vec::new();
    for (i, (g, w)) in got.iter().zip(cases.expected.iter()).enumerate() {
        if g != w {
            mismatches.push(format!(
                "{}: got {g}, want {w} ({})",
                inputs[i],
                diag(inputs[i], g, w)
            ));
            if mismatches.len() >= 20 {
                mismatches.push("... (further mismatches suppressed)".to_string());
                break;
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "softfloat oracle mismatches:\n{}",
        mismatches.join("\n")
    );
}

/// Hex rendering for mismatch diagnostics.
fn diag(input: &str, got: &str, want: &str) -> String {
    let mut parts = input.split(' ');
    let _op = parts.next();
    let a = parts.next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let b = parts.next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    format!(
        "a={:#018x} b={:#018x} got={} want={}",
        a,
        b,
        got.parse::<i64>().map(|v| format!("{v:#018x}")).unwrap_or_else(|_| got.to_string()),
        want.parse::<i64>().map(|v| format!("{v:#018x}")).unwrap_or_else(|_| want.to_string()),
    )
}

#[test]
fn softfloat_f64_arith() {
    let mut cases = Cases::new();
    f64_arith_cases(&mut cases);
    run_cases(&cases);
}

const QNAN32: i64 = 0x7fc00000;

fn f32_of(bits: i64) -> f32 {
    f32::from_bits(bits as u32)
}

fn canon32(x: f32) -> String {
    if x.is_nan() {
        QNAN32.to_string()
    } else {
        x.to_bits().to_string()
    }
}

/// Extra unary vectors around integral boundaries for the rounding ops.
const F64_ROUND_EDGES: &[u64] = &[
    0xbfe0000000000000, // -0.5
    0x4004000000000000, // 2.5
    0xc004000000000000,
    0x3fe6666666666666, // ~0.7
    0xbfe6666666666666,
    0x432fffffffffffff, // 2^52 - 0.5
    0xc32fffffffffffff,
    0x4320000000000001,
    0xc320000000000001,
    0x3ff4000000000000, // 1.25
    0xbff4000000000000,
];

const F32_EDGES: &[u32] = &[
    0x00000000, 0x80000000, // ±0
    0x00000001, 0x80000001, // ±min subnormal
    0x007fffff, 0x807fffff, // ±max subnormal
    0x00800000, 0x80800000, // ±min normal
    0x7f7fffff, 0xff7fffff, // ±max finite
    0x3f800000, 0xbf800000, // ±1
    0x3f800001, 0x3f7fffff, // 1 ± ulp
    0x3fc00000, 0xbfc00000, // ±1.5
    0x40000000, 0x3f000000, // 2, 0.5
    0xbf000000, 0x40200000, // -0.5, 2.5
    0xc0200000, 0x4affffff,
    0x4b000000, 0x4b800000, // 2^23, 2^24
    0x7f800000, 0xff800000, // ±inf
    0x7fc00000, 0x7f800001, 0xffc0dead, // NaNs
];

fn wasm_min64(a: i64, b: i64) -> String {
    let (x, y) = (f64_of(a), f64_of(b));
    if x.is_nan() || y.is_nan() {
        return QNAN64.to_string();
    }
    if x < y {
        a.to_string()
    } else if y < x {
        b.to_string()
    } else {
        (a | b).to_string()
    }
}

fn wasm_max64(a: i64, b: i64) -> String {
    let (x, y) = (f64_of(a), f64_of(b));
    if x.is_nan() || y.is_nan() {
        return QNAN64.to_string();
    }
    if x > y {
        a.to_string()
    } else if y > x {
        b.to_string()
    } else {
        (a & b).to_string()
    }
}

#[test]
fn softfloat_f64_patterns() {
    let mut cases = Cases::new();
    let mut pairs: Vec<(i64, i64)> = Vec::new();
    for &a in F64_EDGES {
        for &b in F64_EDGES {
            pairs.push((a as i64, b as i64));
        }
    }
    let mut rng = Lcg(0x5eed_0002);
    for _ in 0..800 {
        pairs.push((rng.next() as i64, rng.next() as i64));
    }
    for &(a, b) in &pairs {
        let (x, y) = (f64_of(a), f64_of(b));
        cases.push("f64_eq", a, b, ((x == y) as i32).to_string());
        cases.push("f64_ne", a, b, ((x != y) as i32).to_string());
        cases.push("f64_lt", a, b, ((x < y) as i32).to_string());
        cases.push("f64_gt", a, b, ((x > y) as i32).to_string());
        cases.push("f64_le", a, b, ((x <= y) as i32).to_string());
        cases.push("f64_ge", a, b, ((x >= y) as i32).to_string());
        cases.push("f64_min", a, b, wasm_min64(a, b));
        cases.push("f64_max", a, b, wasm_max64(a, b));
    }
    let mut singles: Vec<i64> = F64_EDGES.iter().map(|&v| v as i64).collect();
    singles.extend(F64_ROUND_EDGES.iter().map(|&v| v as i64));
    for _ in 0..1500 {
        singles.push(rng.next() as i64);
    }
    for &a in &singles {
        let x = f64_of(a);
        cases.push("f64_ceil", a, 0, canon64(x.ceil()));
        cases.push("f64_floor", a, 0, canon64(x.floor()));
        cases.push("f64_trunc", a, 0, canon64(x.trunc()));
        cases.push("f64_nearest", a, 0, canon64(x.round_ties_even()));
    }
    run_cases(&cases);
}

fn wasm_min32(a: i64, b: i64) -> String {
    let (x, y) = (f32_of(a), f32_of(b));
    if x.is_nan() || y.is_nan() {
        return QNAN32.to_string();
    }
    if x < y {
        a.to_string()
    } else if y < x {
        b.to_string()
    } else {
        (a | b).to_string()
    }
}

fn wasm_max32(a: i64, b: i64) -> String {
    let (x, y) = (f32_of(a), f32_of(b));
    if x.is_nan() || y.is_nan() {
        return QNAN32.to_string();
    }
    if x > y {
        a.to_string()
    } else if y > x {
        b.to_string()
    } else {
        (a & b).to_string()
    }
}

const INT_EDGES: &[i64] = &[
    0,
    1,
    -1,
    2,
    7,
    0x7fffffff,
    0x80000000,
    0xffffffff,
    0x100000000,
    (1 << 24) - 1,
    1 << 24,
    (1 << 24) + 1,
    (1 << 53) - 1,
    1 << 53,
    (1 << 53) + 1,
    (1 << 53) + 2,
    (1 << 53) + 3,
    i64::MAX,
    i64::MIN,
    i64::MIN + 1,
    -(1 << 53) - 1,
    0x8000000000000001u64 as i64,
    0xfffffffffffffbffu64 as i64,
];

/// f64 patterns straddling the trunc range boundaries.
const TRUNC_EDGES64: &[u64] = &[
    0x41dfffffffffffff, // just under 2^31
    0x41e0000000000000, // 2^31
    0xc1e0000000000000, // -2^31
    0xc1e0000000200000, // past -2^31
    0x41efffffffe00000, // 4294967295.0
    0x41efffffffffffff, // 4294967295.99…
    0x41f0000000000000, // 2^32
    0x43dfffffffffffff,
    0x43e0000000000000, // 2^63
    0xc3e0000000000000, // -2^63
    0xc3e0000000000001, // past -2^63
    0x43efffffffffffff, // just under 2^64
    0x43f0000000000000, // 2^64
    0xbfeccccccccccccd, // -0.9
    0xbff0000000000000, // -1.0
    0x3fe0000000000000, // 0.5
];

const TRUNC_EDGES32: &[u32] = &[
    0x4effffff, // just under 2^31
    0x4f000000, // 2^31
    0xcf000000, // -2^31
    0xcf000001,
    0x4f7fffff, // just under 2^32
    0x4f800000, // 2^32
    0x5effffff,
    0x5f000000, // 2^63
    0xdf000000, // -2^63
    0xdf000001,
    0x5f7fffff,
    0x5f800000, // 2^64
    0xbf666666, // -0.9
    0xbf800000, // -1.0
    0x3f000000, // 0.5
];

/// The Ruby backend's trunc semantics: NaN and out-of-range trap; the
/// bound check is on the truncated value.
fn oracle_trunc(x: f64, lo: f64, hi_excl: f64) -> Result<f64, &'static str> {
    if x.is_nan() {
        return Err("invalid conversion to integer");
    }
    let t = x.trunc();
    if !(t >= lo && t < hi_excl) {
        return Err("integer overflow");
    }
    Ok(t)
}

fn trunc_expect(x: f64, lo: f64, hi_excl: f64, to_pattern: fn(f64) -> i64) -> String {
    match oracle_trunc(x, lo, hi_excl) {
        Ok(t) => to_pattern(t).to_string(),
        Err(msg) => format!("T {msg}"),
    }
}

fn push_trunc_cases(cases: &mut Cases, prefix: &str, a: i64, x: f64) {
    let two31 = 2147483648.0f64;
    let two32 = 4294967296.0f64;
    let two63 = 9223372036854775808.0f64;
    let two64 = 18446744073709551616.0f64;
    cases.push(
        &format!("i32_trunc_{prefix}_s"),
        a,
        0,
        trunc_expect(x, -two31, two31, |t| (t as i64) & 0xffffffff),
    );
    cases.push(
        &format!("i32_trunc_{prefix}_u"),
        a,
        0,
        trunc_expect(x, -0.0, two32, |t| (t as i64) & 0xffffffff),
    );
    cases.push(
        &format!("i64_trunc_{prefix}_s"),
        a,
        0,
        trunc_expect(x, -two63, two63, |t| t as i64),
    );
    cases.push(
        &format!("i64_trunc_{prefix}_u"),
        a,
        0,
        trunc_expect(x, -0.0, two64, |t| t as u64 as i64),
    );
    cases.push(
        &format!("i32_trunc_sat_{prefix}_s"),
        a,
        0,
        ((x as i32) as u32 as i64).to_string(),
    );
    cases.push(&format!("i32_trunc_sat_{prefix}_u"), a, 0, ((x as u32) as i64).to_string());
    cases.push(&format!("i64_trunc_sat_{prefix}_s"), a, 0, (x as i64).to_string());
    cases.push(
        &format!("i64_trunc_sat_{prefix}_u"),
        a,
        0,
        ((x as u64) as i64).to_string(),
    );
}

#[test]
fn softfloat_conversions() {
    let mut cases = Cases::new();
    let mut rng = Lcg(0x5eed_0004);
    let mut ints: Vec<i64> = INT_EDGES.to_vec();
    for _ in 0..1500 {
        ints.push(rng.next() as i64);
    }
    for &v in &ints {
        let u32v = v & 0xffffffff;
        let sv = u32v as u32 as i32;
        cases.push("f64_convert_i32_s", u32v, 0, ((sv as f64).to_bits() as i64).to_string());
        cases.push(
            "f64_convert_i32_u",
            u32v,
            0,
            (((u32v as u32) as f64).to_bits() as i64).to_string(),
        );
        cases.push("f64_convert_i64_s", v, 0, ((v as f64).to_bits() as i64).to_string());
        cases.push(
            "f64_convert_i64_u",
            v,
            0,
            (((v as u64) as f64).to_bits() as i64).to_string(),
        );
        cases.push("f32_convert_i32_s", u32v, 0, ((sv as f32).to_bits() as i64).to_string());
        cases.push(
            "f32_convert_i32_u",
            u32v,
            0,
            (((u32v as u32) as f32).to_bits() as i64).to_string(),
        );
        cases.push("f32_convert_i64_s", v, 0, ((v as f32).to_bits() as i64).to_string());
        cases.push(
            "f32_convert_i64_u",
            v,
            0,
            (((v as u64) as f32).to_bits() as i64).to_string(),
        );
    }

    // promote / demote
    let mut f32s: Vec<i64> = F32_EDGES.iter().map(|&v| v as i64).collect();
    let mut f64s: Vec<i64> = F64_EDGES.iter().map(|&v| v as i64).collect();
    for _ in 0..1500 {
        f32s.push((rng.next() as u32) as i64);
        f64s.push(rng.next() as i64);
    }
    for &p in &f32s {
        let x = f32_of(p);
        let expect = if x.is_nan() {
            QNAN64.to_string()
        } else {
            ((x as f64).to_bits() as i64).to_string()
        };
        cases.push("f64_promote", p, 0, expect);
    }
    for &a in &f64s {
        let x = f64_of(a);
        let expect = if x.is_nan() {
            ((((a >> 63) & 1) << 31) | QNAN32).to_string()
        } else {
            ((x as f32).to_bits() as i64).to_string()
        };
        cases.push("f32_demote", a, 0, expect);
    }

    // truncations, f64 and f32 sources
    let mut trunc64: Vec<i64> = F64_EDGES.iter().map(|&v| v as i64).collect();
    trunc64.extend(TRUNC_EDGES64.iter().map(|&v| v as i64));
    for _ in 0..1000 {
        trunc64.push(rng.next() as i64);
    }
    for &a in &trunc64 {
        push_trunc_cases(&mut cases, "f64", a, f64_of(a));
    }
    let mut trunc32: Vec<i64> = F32_EDGES.iter().map(|&v| v as i64).collect();
    trunc32.extend(TRUNC_EDGES32.iter().map(|&v| v as i64));
    for _ in 0..1000 {
        trunc32.push((rng.next() as u32) as i64);
    }
    for &a in &trunc32 {
        push_trunc_cases(&mut cases, "f32", a, f32_of(a) as f64);
    }
    run_cases(&cases);
}

/// f32 arithmetic through promote/f64/demote: the empirical check of the
/// double-rounding theorem, biased toward the 24-bit boundary.
#[test]
fn softfloat_f32_arith() {
    let mut cases = Cases::new();
    let mut pairs: Vec<(i64, i64)> = Vec::new();
    for &a in F32_EDGES {
        for &b in F32_EDGES {
            pairs.push((a as i64, b as i64));
        }
    }
    let mut rng = Lcg(0x5eed_0005);
    for _ in 0..1500 {
        pairs.push(((rng.next() as u32) as i64, (rng.next() as u32) as i64));
    }
    // Exponent-correlated pairs to stress alignment/cancellation ties.
    for _ in 0..800 {
        let a = (rng.next() as u32) as i64;
        let ea = ((a >> 23) & 0xff).clamp(1, 0xfe);
        let delta = (rng.next() % 61) as i64 - 30;
        let eb = (ea + delta).clamp(0, 0xfe);
        let b = ((rng.next() as u32) as i64 & !(0xffi64 << 23)) | (eb << 23);
        pairs.push((a, b));
    }
    for &(a, b) in &pairs {
        let (x, y) = (f32_of(a), f32_of(b));
        cases.push("f32_add", a, b, canon32(x + y));
        cases.push("f32_sub", a, b, canon32(x - y));
        cases.push("f32_mul", a, b, canon32(x * y));
        cases.push("f32_div", a, b, canon32(x / y));
    }
    for &(a, _) in &pairs {
        cases.push("f32_sqrt", a, 0, canon32(f32_of(a).sqrt()));
    }
    run_cases(&cases);
}

#[test]
fn softfloat_f32_patterns() {
    let mut cases = Cases::new();
    let mut pairs: Vec<(i64, i64)> = Vec::new();
    for &a in F32_EDGES {
        for &b in F32_EDGES {
            pairs.push((a as i64, b as i64));
        }
    }
    let mut rng = Lcg(0x5eed_0003);
    for _ in 0..800 {
        pairs.push(((rng.next() as u32) as i64, (rng.next() as u32) as i64));
    }
    for &(a, b) in &pairs {
        let (x, y) = (f32_of(a), f32_of(b));
        cases.push("f32_eq", a, b, ((x == y) as i32).to_string());
        cases.push("f32_ne", a, b, ((x != y) as i32).to_string());
        cases.push("f32_lt", a, b, ((x < y) as i32).to_string());
        cases.push("f32_gt", a, b, ((x > y) as i32).to_string());
        cases.push("f32_le", a, b, ((x <= y) as i32).to_string());
        cases.push("f32_ge", a, b, ((x >= y) as i32).to_string());
        cases.push("f32_min", a, b, wasm_min32(a, b));
        cases.push("f32_max", a, b, wasm_max32(a, b));
    }
    let mut singles: Vec<i64> = F32_EDGES.iter().map(|&v| v as i64).collect();
    for _ in 0..1500 {
        singles.push((rng.next() as u32) as i64);
    }
    for &a in &singles {
        let x = f32_of(a);
        cases.push("f32_ceil", a, 0, canon32(x.ceil()));
        cases.push("f32_floor", a, 0, canon32(x.floor()));
        cases.push("f32_trunc", a, 0, canon32(x.trunc()));
        cases.push("f32_nearest", a, 0, canon32(x.round_ties_even()));
    }
    run_cases(&cases);
}
