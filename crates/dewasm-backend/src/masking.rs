//! Mask elision inside one expression tree, for the backends that store integers masked-unsigned.
//!
//! The stored representation masks every integer to its width, and each wrapping operation re-masks its result.
//! Inside a single [`Expr`] tree that is often double work: a consumer that reads its operand only modulo 2^32 or 2^64 (a wrapping add's operand, a shift count) cannot observe the high bits its own mask throws away.
//! [`MaskContext`] names the two consumption contexts, [`bin_operand_context`] and [`un_operand_context`] are the table of which operands an operation reads modularly, and [`elides_mask`] is the guard: a backend may skip a site's own result mask exactly when the consumer is modular and the interval bound proves the exposed value stays within the backend's unboxed-integer range.
//! [`shift_count_mode`] is the count-position counterpart: the `& (width - 1)` a backend emits on a shift count implements wasm's semantic reduction, and it folds for a constant count and drops when the count's exact rendering provably sits in range.
//!
//! Soundness rests on the targets' integers being arbitrary-precision two's complement (Ruby, Python, Perl): an unmasked intermediate is congruent to the masked value modulo 2^w, every modular consumer preserves that congruence (a bitwise operator reads a negative operand as its infinite two's-complement form, so `(x - y) & 0xffffffff` is the correct wrap of a negative difference), and the first non-modular consumer sits behind a kept mask, which reduces the value back to the stored representation.

use dewasm_core::ir::{BinOp, Expr, LoadOp, UnOp};

/// How a consumer reads an integer operand: `Masked` when it observes the exact stored value, `Modular` when it reads only the value's congruence class modulo the type width, so an unmasked operand serves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskContext {
    Masked,
    Modular,
}

/// The context in which `op` consumes its operand `k` (0-based), given that `op`'s own result is consumed in `ctx`.
///
/// An operation with its own result mask (wrapping add/sub/mul, `shl`, the wrap) reads its operands modularly regardless of `ctx`: the site's mask restores the invariant even when its own elision guard fails.
/// A shift count is read through the semantic `& (w - 1)`, so it is modular for every shift; a backend that renders counts through [`shift_count_mode`] consults that instead, because a skipped count reduction demands the `Masked` context.
/// The maskless bitwise operators pass `ctx` through: their result is exact only when their operands are.
/// Everything else (comparisons, division, signed and unsigned views, addresses, helper calls) observes the exact value.
pub fn bin_operand_context(op: BinOp, k: usize, ctx: MaskContext) -> MaskContext {
    use BinOp::*;
    match op {
        I32Add | I32Sub | I32Mul | I64Add | I64Sub | I64Mul | I32Shl | I64Shl => {
            MaskContext::Modular
        }
        // The shifted value's high bits are observed exactly; the count is not.
        I32ShrU | I32ShrS | I64ShrU | I64ShrS => {
            if k == 1 {
                MaskContext::Modular
            } else {
                MaskContext::Masked
            }
        }
        I32And | I32Or | I32Xor | I64And | I64Or | I64Xor => ctx,
        _ => MaskContext::Masked,
    }
}

/// The [`bin_operand_context`] counterpart for unary operations: only the wrap consumes modularly (its result mask keeps the low 32 bits either way).
pub fn un_operand_context(op: UnOp) -> MaskContext {
    match op {
        UnOp::I32WrapI64 => MaskContext::Modular,
        _ => MaskContext::Masked,
    }
}

/// Whether `e`'s own result mask may be skipped when its consumer is modular: `e` must carry a mask of its own, and the value it exposes unmasked must provably stay in `[-limit, limit)`.
/// `limit` is the backend's unboxed-integer bound (Ruby: `1 << 62`); the guard keeps elision strictly profitable by never exposing an intermediate the mask would have kept out of bignum arithmetic.
pub fn elides_mask(e: &Expr, limit: i128) -> bool {
    match raw_bound(e, limit) {
        Some(raw) => raw.within(limit),
        None => false,
    }
}

/// How a backend renders a shift count.
/// wasm reduces the count modulo the shift width; the `& (bits - 1)` a backend emits implements that reduction and is dropped exactly when the reduction is provably the identity on the rendered value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShiftCountMode {
    /// A constant count, already reduced modulo the width: emit the value bare.
    Constant(u32),
    /// The count's `Masked` rendering provably sits in `0..bits`: emit that rendering bare.
    InRange,
    /// Emit the count's `Modular` rendering under `& (bits - 1)`.
    Masked,
}

/// [`ShiftCountMode`] for the count `e` of a `bits`-wide shift, under the elision policy of [`elides_mask`] with the same `limit`.
///
/// The emitted `& (bits - 1)` is congruence-preserving, so a kept reduction takes the count's `Modular` rendering.
/// Skipping it hands the rendered value straight to the target's shift operator, which observes the exact count (a negative or oversized count would shift the wrong way or too far), so the in-range proof is judged on the `Masked` rendering, and an `InRange` site must emit that rendering.
pub fn shift_count_mode(e: &Expr, bits: u32, limit: i128) -> ShiftCountMode {
    if matches!(e, Expr::I32Const(_) | Expr::I64Const(_)) {
        return ShiftCountMode::Constant(count_bound(e, bits).0);
    }
    let b = bound(e, bits, MaskContext::Masked, limit);
    if b.lo >= 0 && b.hi < i128::from(bits) {
        ShiftCountMode::InRange
    } else {
        ShiftCountMode::Masked
    }
}

/// The width `op` reduces its shift count modulo, for the shifts whose count reduction sits at the call site (`rotl`/`rotr` reduce inside their runtime helpers).
pub fn shift_width(op: BinOp) -> Option<u32> {
    use BinOp::*;
    match op {
        I32Shl | I32ShrS | I32ShrU => Some(32),
        I64Shl | I64ShrS | I64ShrU => Some(64),
        _ => None,
    }
}

/// Everything the interval analysis tracks is clamped to this magnitude: far beyond any elision limit, and small enough that saturating i128 arithmetic on two clamped bounds cannot wrap.
const CEILING: i128 = 1 << 100;

#[derive(Clone, Copy, Debug)]
struct Bound {
    lo: i128,
    hi: i128,
}

impl Bound {
    fn new(lo: i128, hi: i128) -> Bound {
        Bound {
            lo: lo.clamp(-CEILING, CEILING),
            hi: hi.clamp(-CEILING, CEILING),
        }
    }

    fn exact(v: i128) -> Bound {
        Bound::new(v, v)
    }

    /// The full masked range of a `bits`-wide value: `[0, 2^bits)`.
    fn masked(bits: u32) -> Bound {
        Bound {
            lo: 0,
            hi: (1i128 << bits) - 1,
        }
    }

    fn union(self, o: Bound) -> Bound {
        Bound {
            lo: self.lo.min(o.lo),
            hi: self.hi.max(o.hi),
        }
    }

    fn within(self, limit: i128) -> bool {
        self.lo >= -limit && self.hi < limit
    }

    fn is_masked_range(self, bits: u32) -> bool {
        self.lo >= 0 && self.hi < (1i128 << bits)
    }
}

/// The interval of the value `e`'s rendering takes when consumed in `ctx`, under the elision policy of [`elides_mask`] with the same `limit`.
/// `bits` is the width of `e`'s integer type, which the caller knows from the operation consuming it.
fn bound(e: &Expr, bits: u32, ctx: MaskContext, limit: i128) -> Bound {
    if let Some(raw) = raw_bound(e, limit) {
        if ctx == MaskContext::Modular && raw.within(limit) {
            return raw;
        }
        // The kept mask reduces modulo 2^bits: exactly `raw` when no value in it wraps.
        return if raw.is_masked_range(bits) {
            raw
        } else {
            Bound::masked(bits)
        };
    }
    use MaskContext::Masked;
    match e {
        Expr::I32Const(v) => Bound::exact(i128::from(*v)),
        Expr::I64Const(v) => Bound::exact(i128::from(*v)),
        Expr::Temp(_) | Expr::LocalGet(_) | Expr::GlobalGet(_) => Bound::masked(bits),
        // wasm's memory is capped at 65536 pages.
        Expr::MemorySize => Bound::new(0, 1 << 16),
        Expr::Load { op, .. } => load_bound(*op, bits),
        Expr::Select { then, els, .. } => {
            bound(then, bits, Masked, limit).union(bound(els, bits, Masked, limit))
        }
        Expr::Un(op, a) => match op {
            UnOp::I32Eqz | UnOp::I64Eqz => Bound::new(0, 1),
            UnOp::I32Clz | UnOp::I32Ctz | UnOp::I32Popcnt => Bound::new(0, 32),
            UnOp::I64Clz | UnOp::I64Ctz | UnOp::I64Popcnt => Bound::new(0, 64),
            UnOp::I64ExtendI32U => bound(a, 32, Masked, limit),
            _ => Bound::masked(bits),
        },
        Expr::Bin(op, a, b) => {
            use BinOp::*;
            if crate::comparison(*op).is_some() {
                return Bound::new(0, 1);
            }
            match op {
                I32And | I64And | I32Or | I64Or | I32Xor | I64Xor => bitwise_bound(
                    matches!(op, I32And | I64And),
                    bound(a, bits, ctx, limit),
                    bound(b, bits, ctx, limit),
                ),
                I32ShrU | I64ShrU => {
                    let ba = bound(a, bits, Masked, limit);
                    let (cmin, cmax) = count_bound(b, bits);
                    Bound::new(
                        (ba.lo >> cmin).min(ba.lo >> cmax),
                        (ba.hi >> cmin).max(ba.hi >> cmax),
                    )
                }
                _ => Bound::masked(bits),
            }
        }
        Expr::F32Const(_) | Expr::F64Const(_) => Bound::masked(bits),
    }
}

/// The interval of `e`'s value rendered without its own result mask, or `None` when `e` carries no mask of its own.
fn raw_bound(e: &Expr, limit: i128) -> Option<Bound> {
    use BinOp::*;
    use MaskContext::Modular;
    match e {
        Expr::Un(UnOp::I32WrapI64, a) => Some(bound(a, 64, Modular, limit)),
        Expr::Bin(op, a, b) => {
            let bits = match op {
                I32Add | I32Sub | I32Mul | I32Shl | I32ShrS => 32,
                I64Add | I64Sub | I64Mul | I64Shl | I64ShrS => 64,
                _ => return None,
            };
            let ba = bound(a, bits, bin_operand_context(*op, 0, Modular), limit);
            let bb = |b: &Expr| bound(b, bits, Modular, limit);
            Some(match op {
                I32Add | I64Add => {
                    let bb = bb(b);
                    Bound::new(ba.lo.saturating_add(bb.lo), ba.hi.saturating_add(bb.hi))
                }
                I32Sub | I64Sub => {
                    let bb = bb(b);
                    Bound::new(ba.lo.saturating_sub(bb.hi), ba.hi.saturating_sub(bb.lo))
                }
                I32Mul | I64Mul => {
                    let bb = bb(b);
                    let c = [
                        ba.lo.saturating_mul(bb.lo),
                        ba.lo.saturating_mul(bb.hi),
                        ba.hi.saturating_mul(bb.lo),
                        ba.hi.saturating_mul(bb.hi),
                    ];
                    Bound::new(*c.iter().min().unwrap(), *c.iter().max().unwrap())
                }
                I32Shl | I64Shl => {
                    let (cmin, cmax) = count_bound(b, bits);
                    let shl = |v: i128, c: u32| v.saturating_mul(1i128 << c);
                    let c = [
                        shl(ba.lo, cmin),
                        shl(ba.lo, cmax),
                        shl(ba.hi, cmin),
                        shl(ba.hi, cmax),
                    ];
                    Bound::new(*c.iter().min().unwrap(), *c.iter().max().unwrap())
                }
                I32ShrS | I64ShrS => {
                    let s = signed_view(ba, bits);
                    let (cmin, cmax) = count_bound(b, bits);
                    let c = [s.lo >> cmin, s.lo >> cmax, s.hi >> cmin, s.hi >> cmax];
                    Bound::new(*c.iter().min().unwrap(), *c.iter().max().unwrap())
                }
                _ => unreachable!("filtered by the bits match above"),
            })
        }
        _ => None,
    }
}

/// The values a shift count takes after its semantic `& (bits - 1)`: exact for a constant, the full `0..bits` otherwise.
fn count_bound(b: &Expr, bits: u32) -> (u32, u32) {
    match b {
        Expr::I32Const(v) => {
            let c = v & (bits - 1);
            (c, c)
        }
        Expr::I64Const(v) => {
            let c = (*v & u64::from(bits - 1)) as u32;
            (c, c)
        }
        _ => (0, bits - 1),
    }
}

/// The interval of the signed view (`s32`/`s64`) of a masked interval: the identity below the sign bit, the full signed range once the interval reaches it.
fn signed_view(a: Bound, bits: u32) -> Bound {
    let half = 1i128 << (bits - 1);
    if a.lo >= 0 && a.hi < half {
        a
    } else {
        Bound::new(-half, half - 1)
    }
}

/// Bitwise `&`, `|`, `^` on two's-complement integers.
/// `x & y` with a non-negative `y` never sets a bit `y` lacks, whatever `x`'s sign: `0 <= x & y <= y`.
/// `|` and `^` of non-negative operands stay inside the operands' bit envelope; with a possibly negative operand, anything representable in the covering two's-complement width can come out.
fn bitwise_bound(is_and: bool, a: Bound, b: Bound) -> Bound {
    if is_and {
        match (a.lo >= 0, b.lo >= 0) {
            (true, true) => return Bound::new(0, a.hi.min(b.hi)),
            (true, false) => return Bound::new(0, a.hi),
            (false, true) => return Bound::new(0, b.hi),
            (false, false) => {}
        }
    } else if a.lo >= 0 && b.lo >= 0 {
        let h = a.hi.max(b.hi);
        let bits = 128 - (h as u128).leading_zeros();
        return if bits >= 100 {
            Bound::new(0, CEILING)
        } else {
            Bound::new(0, (1i128 << bits) - 1)
        };
    }
    let mut p = 1i128;
    while p < CEILING && (a.lo < -p || a.hi >= p || b.lo < -p || b.hi >= p) {
        p <<= 1;
    }
    Bound::new(-p, p - 1)
}

/// The masked range a load produces, exact for the zero-extending narrow loads; a sign-extending load is stored re-masked, so it spans the full destination width.
fn load_bound(op: LoadOp, bits: u32) -> Bound {
    use LoadOp::*;
    match op {
        I32Load8U | I64Load8U => Bound::new(0, 0xff),
        I32Load16U | I64Load16U => Bound::new(0, 0xffff),
        I64Load32U => Bound::new(0, 0xffff_ffff),
        I32Load => Bound::masked(32),
        I64Load => Bound::masked(64),
        _ => Bound::masked(bits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: i128 = 1 << 62;

    fn local(idx: u32) -> Expr {
        Expr::LocalGet(idx)
    }

    fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
        Expr::Bin(op, Box::new(a), Box::new(b))
    }

    fn load8u() -> Expr {
        Expr::Load {
            op: LoadOp::I64Load8U,
            addr: Box::new(local(0)),
            offset: 0,
        }
    }

    #[test]
    fn i32_add_and_sub_of_full_range_operands_elide() {
        assert!(elides_mask(&bin(BinOp::I32Add, local(0), local(1)), LIMIT));
        assert!(elides_mask(&bin(BinOp::I32Sub, local(0), local(1)), LIMIT));
    }

    #[test]
    fn i32_mul_of_full_range_operands_keeps_its_mask() {
        // (2^32 - 1)^2 exceeds the Fixnum limit.
        assert!(!elides_mask(&bin(BinOp::I32Mul, local(0), local(1)), LIMIT));
    }

    #[test]
    fn i32_mul_of_narrowed_operands_elides() {
        let byte = |idx| bin(BinOp::I32And, local(idx), Expr::I32Const(0xff));
        assert!(elides_mask(&bin(BinOp::I32Mul, byte(0), byte(1)), LIMIT));
    }

    #[test]
    fn and_bounds_by_a_non_negative_operand_alone() {
        // The unmasked difference can be negative, but `& 0xff` still lands in 0..255, so the product elides.
        let byte_of_diff = |i, j| {
            bin(
                BinOp::I32And,
                bin(BinOp::I32Sub, local(i), local(j)),
                Expr::I32Const(0xff),
            )
        };
        assert!(elides_mask(
            &bin(BinOp::I32Mul, byte_of_diff(0, 1), byte_of_diff(2, 3)),
            LIMIT
        ));
    }

    #[test]
    fn i32_shr_s_always_elides() {
        // The raw signed value is within [-2^31, 2^31), well inside the limit.
        assert!(elides_mask(&bin(BinOp::I32ShrS, local(0), local(1)), LIMIT));
    }

    #[test]
    fn shl_elides_only_under_a_small_count_or_operand() {
        // Full range shifted by an unknown count reaches 2^63.
        assert!(!elides_mask(&bin(BinOp::I32Shl, local(0), local(1)), LIMIT));
        // An exact small count keeps the raw value within the limit.
        assert!(elides_mask(
            &bin(BinOp::I32Shl, local(0), Expr::I32Const(4)),
            LIMIT
        ));
    }

    #[test]
    fn wrap_elides_only_for_a_narrow_i64() {
        let wrap = |e: Expr| Expr::Un(UnOp::I32WrapI64, Box::new(e));
        // A full-range i64 exceeds the limit, so the wrap's mask stays.
        assert!(!elides_mask(&wrap(local(0)), LIMIT));
        assert!(elides_mask(&wrap(load8u()), LIMIT));
        // The high half extracted by `>> 32` is bounded by 2^32.
        assert!(elides_mask(
            &wrap(bin(BinOp::I64ShrU, local(0), Expr::I64Const(32))),
            LIMIT
        ));
    }

    #[test]
    fn i64_arithmetic_elides_only_with_provably_narrow_operands() {
        assert!(!elides_mask(&bin(BinOp::I64Add, local(0), local(1)), LIMIT));
        assert!(!elides_mask(
            &bin(BinOp::I64ShrS, local(0), local(1)),
            LIMIT
        ));
        assert!(elides_mask(
            &bin(BinOp::I64Add, load8u(), Expr::I64Const(1)),
            LIMIT
        ));
    }

    #[test]
    fn nested_elision_bounds_compose() {
        // (l0 + l1) + l2 unmasked is below 3 * 2^32.
        let inner = bin(BinOp::I32Add, local(0), local(1));
        assert!(elides_mask(&bin(BinOp::I32Add, inner, local(2)), LIMIT));
        // ((l0 + l1) * l2) unmasked reaches 2^65: the mul keeps its mask even though its operand elides.
        let inner = bin(BinOp::I32Add, local(0), local(1));
        assert!(!elides_mask(&bin(BinOp::I32Mul, inner, local(2)), LIMIT));
    }

    #[test]
    fn shift_count_folds_a_constant_reduced_modulo_the_width() {
        use ShiftCountMode::Constant;
        assert_eq!(shift_count_mode(&Expr::I32Const(2), 32, LIMIT), Constant(2));
        // The width itself reduces to 0; the shift still happens, by nothing.
        assert_eq!(
            shift_count_mode(&Expr::I32Const(32), 32, LIMIT),
            Constant(0)
        );
        // 32 and 63 are valid i64 counts and stay themselves.
        assert_eq!(
            shift_count_mode(&Expr::I64Const(32), 64, LIMIT),
            Constant(32)
        );
        assert_eq!(
            shift_count_mode(&Expr::I64Const(63), 64, LIMIT),
            Constant(63)
        );
        // A wrapped negative reduces like any other bit pattern.
        assert_eq!(
            shift_count_mode(&Expr::I32Const(-3i32 as u32), 32, LIMIT),
            Constant(29)
        );
    }

    #[test]
    fn shift_count_drops_the_reduction_only_for_a_provably_in_range_count() {
        use ShiftCountMode::{InRange, Masked};
        // The doubled reduction wasm code produces: the count `l0 & 63` already sits in 0..64.
        assert_eq!(
            shift_count_mode(&bin(BinOp::I64And, local(0), Expr::I64Const(63)), 64, LIMIT),
            InRange
        );
        // A full-range count keeps the reduction.
        assert_eq!(shift_count_mode(&local(0), 64, LIMIT), Masked);
        // `clz` reaches the width itself (32 on an i32 zero), one past the last valid count.
        assert_eq!(
            shift_count_mode(&Expr::Un(UnOp::I32Clz, Box::new(local(0))), 32, LIMIT),
            Masked
        );
        // A sub elides its mask under a modular consumer, but its exact rendering can be negative: the in-range proof binds the Masked rendering, so the reduction stays.
        assert_eq!(
            shift_count_mode(&bin(BinOp::I32Sub, local(0), local(1)), 32, LIMIT),
            Masked
        );
    }

    #[test]
    fn operand_contexts_follow_the_table() {
        use MaskContext::*;
        assert_eq!(bin_operand_context(BinOp::I32Add, 0, Masked), Modular);
        assert_eq!(bin_operand_context(BinOp::I32ShrU, 0, Modular), Masked);
        assert_eq!(bin_operand_context(BinOp::I32ShrU, 1, Masked), Modular);
        assert_eq!(bin_operand_context(BinOp::I32And, 0, Modular), Modular);
        assert_eq!(bin_operand_context(BinOp::I32And, 0, Masked), Masked);
        assert_eq!(bin_operand_context(BinOp::I32DivU, 0, Modular), Masked);
        assert_eq!(un_operand_context(UnOp::I32WrapI64), Modular);
        assert_eq!(un_operand_context(UnOp::I64ExtendI32U), Masked);
    }
}
