//! Spinel backend: the Ruby backend's output for a host with 64-bit wrapping integers.
//!
//! Spinel compiles Ruby ahead of time, and its `Integer` is one machine word.
//! Under `spinel --int-overflow=wrap`, `+`, `-`, `*` and `<<` wrap at 64 bits.
//! So an i64 is a signed 64-bit word here, and a wrapping operation is the host's own.
//! The Ruby backend holds an i64 as `0..=2**64 - 1` instead, which needs arbitrary precision.
//!
//! Everything but the i64 representation is the Ruby backend's.
//! This crate chooses its [`I64Repr::SignedWord`] dialect.
//! It replaces the runtime units that read or build a 64-bit pattern (see `units/`).
//!
//! The output runs only as a Spinel build with `--int-overflow=wrap`.
//! The runtime checks the wrap when it loads, and raises under any other host.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use anyhow::Result;
use dewasm_backend::{Backend, GenOptions, OutputFile, RuntimeBundler, SupportStatus};
use dewasm_backend_ruby::{Dialect, I64Repr, RubyBackend};
use dewasm_core::feature::Feature;
use dewasm_core::ir::Module;

pub use dewasm_backend_ruby::ruby_string;

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// The flags a Spinel build of this backend's output needs.
pub const SPINEL_FLAGS: &[&str] = &["--int-overflow=wrap"];

/// The Ruby backend's units, with this crate's `units/` in place of the ones of the same name.
pub fn bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| {
        let mut sources: BTreeMap<&str, &str> =
            dewasm_backend_ruby::UNIT_SOURCES.iter().copied().collect();
        sources.extend(UNIT_SOURCES.iter().copied());
        let sources: Vec<(&str, &str)> = sources.into_iter().collect();
        dewasm_backend_ruby::bundler_of(sources.leak())
    })
}

/// The Ruby backend's dialect this backend emits.
pub fn dialect() -> Dialect<'static> {
    Dialect {
        backend: &SpinelBackend,
        bundler: bundler(),
        i64: I64Repr::SignedWord,
        folds_float_identities: true,
    }
}

/// Locate a `spinel` compiler: `$DEWASM_SPINEL`, then `spinel` on `PATH`.
pub fn find_spinel() -> Option<std::path::PathBuf> {
    static SPINEL: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    SPINEL
        .get_or_init(|| {
            let candidate = std::env::var_os("DEWASM_SPINEL")
                .map_or_else(|| "spinel".into(), std::path::PathBuf::from);
            std::process::Command::new(&candidate)
                .arg("--version")
                .output()
                .is_ok_and(|out| out.status.success())
                .then_some(candidate)
        })
        .clone()
}

pub struct SpinelBackend;

impl Backend for SpinelBackend {
    fn name(&self) -> &str {
        "spinel"
    }

    fn file_extension(&self) -> &str {
        "rb"
    }

    fn has_wasi_p1(&self, name: &str) -> bool {
        bundler().has_unit(&format!("wasi/{name}"))
    }

    /// The control flow and the linking are the Ruby backend's, so its declarations hold here.
    fn feature_status(&self, feature: Feature) -> SupportStatus {
        RubyBackend.feature_status(feature)
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
        dewasm_backend_ruby::generate(&dialect(), module, opts)
    }
}

#[cfg(test)]
mod units {
    use super::*;

    #[test]
    fn all_units_bundle() {
        bundler().bundle_all(0).expect("full bundle resolves");
    }

    /// A unit of this crate either replaces a unit of the Ruby backend or adds a new one.
    /// Every other unit comes from the Ruby backend unchanged.
    #[test]
    fn replaced_units_exist_in_the_bundle() {
        for (id, _) in UNIT_SOURCES {
            assert!(bundler().has_unit(id), "{id} is missing from the bundle");
        }
    }
}

/// Shape checks for the signed-word i64 lowering.
/// The specification harness proves the generated code runs right.
/// These check that no i64 result is masked, and which form each unsigned reading takes.
#[cfg(test)]
mod signed_words {
    use super::*;
    use dewasm_backend::RuntimeLinkage;

    /// One function of two i64 parameters whose body is `expr`, of result type `result`.
    /// The body is stored into a local so folding cannot drop it.
    fn expr(result: &str, expr: &str) -> String {
        let wat = format!(
            "(module (func (export \"f\") (param i64 i64) (result {result}) (local {result}) (local.set 2 {expr}) (local.get 2)))"
        );
        let bytes = wat::parse_str(&wat).expect("parse wat");
        let module = dewasm_core::build_module(&bytes).expect("build module");
        let (src, _) = dewasm_backend_ruby::generate_class_with_units_in(
            &dialect(),
            &module,
            "M",
            &RuntimeLinkage::Embedded,
            false,
        )
        .unwrap();
        src
    }

    fn assert_line(src: &str, want: &str) {
        assert!(
            src.lines().any(|l| l.trim() == want),
            "expected line `{want}` in:\n{src}"
        );
    }

    #[test]
    fn wrapping_operations_carry_no_mask() {
        let add = expr("i64", "(i64.add (local.get 0) (local.get 1))");
        assert_line(&add, "l2 = l0 + l1");
        let mul = expr(
            "i64",
            "(i64.mul (local.get 0) (i64.const 6364136223846793005))",
        );
        assert_line(&mul, "l2 = l0 * 6364136223846793005");
        let shl = expr("i64", "(i64.shl (local.get 0) (i64.const 13))");
        assert_line(&shl, "l2 = l0 << 13");
        assert!(!add.contains("m64"), "no mask helper is bundled:\n{add}");
    }

    #[test]
    fn a_constant_is_its_signed_word() {
        let src = expr(
            "i64",
            "(i64.xor (local.get 0) (i64.const 0xffffffffffffffff))",
        );
        assert_line(&src, "l2 = l0 ^ -1");
        let min = expr(
            "i64",
            "(i64.or (local.get 0) (i64.const 0x8000000000000000))",
        );
        assert_line(&min, "l2 = l0 | -9223372036854775808");
    }

    #[test]
    fn a_signed_shift_and_compare_are_the_hosts_own() {
        let shr = expr("i64", "(i64.shr_s (local.get 0) (i64.const 3))");
        assert_line(&shr, "l2 = l0 >> 3");
        let lt = expr("i32", "(i64.lt_s (local.get 0) (local.get 1))");
        assert_line(&lt, "l2 = l0 < l1 ? 1 : 0");
    }

    #[test]
    fn an_unsigned_compare_flips_the_sign_bit() {
        // Ruby's `^` binds tighter than its comparisons, so the flips need no parentheses.
        let lt = expr("i32", "(i64.lt_u (local.get 0) (local.get 1))");
        assert_line(
            &lt,
            "l2 = l0 ^ -9223372036854775808 < l1 ^ -9223372036854775808 ? 1 : 0",
        );
        // A constant side folds: 10 with its sign bit flipped.
        let ge = expr("i32", "(i64.ge_u (local.get 0) (i64.const 10))");
        assert_line(
            &ge,
            "l2 = l0 ^ -9223372036854775808 >= -9223372036854775798 ? 1 : 0",
        );
    }

    #[test]
    fn a_logical_shift_clears_the_copied_sign_bits() {
        let constant = expr("i64", "(i64.shr_u (local.get 0) (i64.const 31))");
        assert_line(&constant, "l2 = l0 >> 31 & 8589934591");
        let variable = expr("i64", "(i64.shr_u (local.get 0) (local.get 1))");
        assert_line(&variable, "l2 = i64_shr_u(l0, l1 & 63)");
    }

    /// Spinel's C compiler folds `x * 1.0` to `x`, which leaves a signaling NaN as it is.
    #[test]
    fn a_float_operation_with_a_constant_quiets_its_result() {
        let wat = "(module (func (export \"f\") (param f64 f64) (result f64) (local f64) (local.set 2 (f64.mul (local.get 0) (f64.const 1))) (local.get 2)) (func (export \"g\") (param f64 f64) (result f64) (local f64) (local.set 2 (f64.mul (local.get 0) (local.get 1))) (local.get 2)))";
        let bytes = wat::parse_str(wat).expect("parse wat");
        let module = dewasm_core::build_module(&bytes).expect("build module");
        let (src, _) = dewasm_backend_ruby::generate_class_with_units_in(
            &dialect(),
            &module,
            "M",
            &RuntimeLinkage::Embedded,
            false,
        )
        .unwrap();
        assert_line(&src, "l2 = (r = l0 * 1.0) == r ? r : quiet_nan(r)");
        assert_line(&src, "l2 = l0 * l1");
    }
}
