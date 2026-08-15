//! Mask elision inside one expression tree, for the backends that store integers masked-unsigned.
//!
//! The stored representation masks every integer to its width, and each wrapping operation re-masks its result.
//! Inside a single [`Expr`] tree that is often double work: a consumer that reads its operand only modulo 2^32 or 2^64 (a wrapping add's operand, a shift count) cannot observe the high bits its own mask throws away.
//! [`MaskContext`] names the consumption contexts, [`bin_operand_context`] and [`un_operand_context`] are the table of which operands an operation reads modularly, and [`elides_mask`] is the guard: a backend may skip a site's own result mask exactly when the consumer's context and the interval bound allow it.
//! [`fold_and_chain`] folds the constants of an AND chain at conversion time, and [`eq_const_rewrite`] drops the mask of a site compared for equality against a constant when the interval pins a unique raw preimage.
//! [`shift_count_mode`] is the count-position counterpart: the `& (width - 1)` a backend emits on a shift count implements wasm's semantic reduction, and it folds for a constant count and drops when the count's exact rendering provably sits in range.
//!
//! Soundness rests on the targets' integers being arbitrary-precision two's complement (Ruby, Python, Perl): an unmasked intermediate is congruent to the masked value modulo 2^w, every modular consumer preserves that congruence (a bitwise operator reads a negative operand as its infinite two's-complement form, so `(x - y) & 0xffffffff` is the correct wrap of a negative difference), and the first non-modular consumer sits behind a kept mask, which reduces the value back to the stored representation.
//! A mask whose raw interval already sits inside `[0, 2^w)` is the identity on the exact value, so it drops in every context.

use dewasm_core::ir::{BinOp, Expr, LoadOp, UnOp};

/// How a consumer reads an integer operand: `Masked` when it observes the exact stored value, `Modular` when it reads only the value's congruence class modulo the type width, so an unmasked operand serves, and `Reducing` when it additionally reduces what it is handed at least as strongly as the operand's own mask would (a bitwise AND with a constant operand, or the pinned equality of [`eq_const_rewrite`]), so the operand's mask drops with no bound guard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskContext {
    Masked,
    Modular,
    Reducing,
}

/// The context in which `op` consumes its operand `k` (0-based), whose sibling operand is `sibling`, given that `op`'s own result is consumed in `ctx`.
///
/// An operation with its own result mask (wrapping add/sub/mul, `shl`, the wrap) reads its operands modularly regardless of `ctx`: the site's mask restores the invariant even when its own elision guard fails.
/// A shift count is read through the semantic `& (w - 1)`, so it is modular for every shift; a backend that renders counts through [`shift_count_mode`] consults that instead, because a skipped count reduction demands the `Masked` context.
/// An AND with a constant sibling reduces its other operand into `[0, constant]` itself (every IR constant sits inside its width), at least as strong a reduction as the operand's own mask, in every `ctx`.
/// The other maskless bitwise operators pass `ctx` through, except that a reducing consumer weakens to `Modular`: it reduces the operator's result, which needs only congruent operands, and the raw operands feed the operator itself, so the bound guard must still hold for them.
/// Everything else (comparisons, division, signed and unsigned views, addresses, helper calls) observes the exact value.
pub fn bin_operand_context(op: BinOp, k: usize, sibling: &Expr, ctx: MaskContext) -> MaskContext {
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
        I32And | I64And if int_const(sibling).is_some() => MaskContext::Reducing,
        I32And | I32Or | I32Xor | I64And | I64Or | I64Xor => match ctx {
            MaskContext::Reducing => MaskContext::Modular,
            _ => ctx,
        },
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

/// Whether `e`'s own result mask may be skipped when its consumer reads it in `ctx`: `e` must carry a mask of its own, and the interval of the value it exposes unmasked decides per [`may_skip_mask`].
/// `limit` is the backend's unboxed-integer bound (Ruby: `1 << 62`).
pub fn elides_mask(e: &Expr, ctx: MaskContext, limit: i128) -> bool {
    match raw_bound(e, limit) {
        Some((raw, bits)) => may_skip_mask(raw, bits, ctx, limit),
        None => false,
    }
}

/// Whether a site with raw interval `raw` under a `bits`-wide mask may render unmasked when consumed in `ctx`.
/// An interval inside `[0, 2^bits)` makes the mask the identity on the exact value: it drops in every context.
/// A modular consumer needs only congruence, but elision must stay strictly profitable: the exposed value must provably stay in `[-limit, limit)`, never trading a cheap mask for bignum arithmetic.
/// A reducing consumer hands the raw value to nothing but its own reduction, which the mask's presence would not weaken, so no guard applies.
fn may_skip_mask(raw: Bound, bits: u32, ctx: MaskContext, limit: i128) -> bool {
    raw.is_masked_range(bits)
        || match ctx {
            MaskContext::Masked => false,
            MaskContext::Modular => raw.within(limit),
            MaskContext::Reducing => true,
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
    if let Some((raw, _)) = raw_bound(e, limit) {
        return if may_skip_mask(raw, bits, ctx, limit) {
            raw
        } else {
            // The kept mask reduces modulo 2^bits.
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
                    bound(a, bits, bin_operand_context(*op, 0, b, ctx), limit),
                    bound(b, bits, bin_operand_context(*op, 1, a, ctx), limit),
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

/// The interval of `e`'s value rendered without its own result mask and the width of that mask, or `None` when `e` carries no mask of its own.
fn raw_bound(e: &Expr, limit: i128) -> Option<(Bound, u32)> {
    use BinOp::*;
    use MaskContext::Modular;
    match e {
        Expr::Un(UnOp::I32WrapI64, a) => Some((bound(a, 64, Modular, limit), 32)),
        Expr::Bin(op, a, b) => {
            let bits = match op {
                I32Add | I32Sub | I32Mul | I32Shl | I32ShrS => 32,
                I64Add | I64Sub | I64Mul | I64Shl | I64ShrS => 64,
                _ => return None,
            };
            let ba = bound(a, bits, bin_operand_context(*op, 0, b, Modular), limit);
            let bb = |b: &Expr| bound(b, bits, Modular, limit);
            let raw = match op {
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
            };
            Some((raw, bits))
        }
        _ => None,
    }
}

/// The value of an integer constant, in its stored masked-unsigned form.
fn int_const(e: &Expr) -> Option<u64> {
    match e {
        Expr::I32Const(v) => Some(u64::from(*v)),
        Expr::I64Const(v) => Some(*v),
        _ => None,
    }
}

/// The conversion-time fold of a constant AND chain: in `x & c1 & c2` the two constants meet in one `c1 & c2`, by associativity.
/// `a` and `b` are the operands of the outermost AND; `Some` only when at least two constants fold.
/// The residual expression is consumed in [`MaskContext::Reducing`], like the non-constant operand of any AND with a constant.
pub fn fold_and_chain<'a>(a: &'a Expr, b: &'a Expr) -> Option<(&'a Expr, u64)> {
    let (mut e, mut c) = and_const_split(a, b)?;
    let mut folded = false;
    while let Expr::Bin(BinOp::I32And | BinOp::I64And, x, y) = e {
        match and_const_split(x, y) {
            Some((inner, c2)) => {
                e = inner;
                c &= c2;
                folded = true;
            }
            None => break,
        }
    }
    folded.then_some((e, c))
}

/// The non-constant operand of an AND and its constant sibling, when exactly one operand is an integer constant.
fn and_const_split<'a>(a: &'a Expr, b: &'a Expr) -> Option<(&'a Expr, u64)> {
    match (int_const(a), int_const(b)) {
        (Some(c), None) => Some((b, c)),
        (None, Some(c)) => Some((a, c)),
        _ => None,
    }
}

/// The unmasked form of an integer equality between a mask site `e` and the constant `c` (given in its stored masked form): `wrap(v) == c` holds exactly when raw `v` lands on `c + k * 2^bits`, so when the raw interval of `v` meets exactly one such candidate, the mask drops and `v` compares against that candidate directly.
/// The constant then migrates across the raw add/sub-by-constant layers the rendering exposes (`x - c1 == c'` is `x == c' + c1`), stopping at the first layer whose own mask stays.
/// The result is the expression to render, the context to render it in, and the exact comparison constant.
/// `None` keeps the mask: when `e` carries none, when several candidates fit the interval, and also when none does; the comparison is then statically false, but the operand can hold a trapping load or division, so it must still run.
pub fn eq_const_rewrite(e: &Expr, c: u64, limit: i128) -> Option<(&Expr, MaskContext, i128)> {
    let (raw, bits) = raw_bound(e, limit)?;
    // An interval touching the analysis ceiling may have been clamped, hiding candidates beyond it.
    if raw.lo <= -CEILING || raw.hi >= CEILING {
        return None;
    }
    let module = 1i128 << bits;
    let c = i128::from(c);
    let k_min = (raw.lo - c + module - 1).div_euclid(module);
    let k_max = (raw.hi - c).div_euclid(module);
    if k_min != k_max {
        return None;
    }
    let mut e = e;
    let mut target = c + k_min * module;
    let mut ctx = MaskContext::Reducing;
    // Invariant: `e` rendered in `ctx` is the raw arithmetic form the algebra below rewrites (at the top the rewrite itself replaces the mask; deeper, elision is checked before descending).
    loop {
        use BinOp::*;
        let peeled = match e {
            Expr::Un(UnOp::I32WrapI64, x) => Some((&**x, target)),
            Expr::Bin(I32Add | I64Add, x, y) => match (int_const(x), int_const(y)) {
                (Some(c1), None) => Some((&**y, target - i128::from(c1))),
                (None, Some(c1)) => Some((&**x, target - i128::from(c1))),
                _ => None,
            },
            Expr::Bin(I32Sub | I64Sub, x, y) => match (int_const(x), int_const(y)) {
                // `c1 - v == t` pins `v == c1 - t`.
                (Some(c1), None) => Some((&**y, i128::from(c1) - target)),
                (None, Some(c1)) => Some((&**x, target + i128::from(c1))),
                _ => None,
            },
            _ => None,
        };
        let Some((inner, t)) = peeled else { break };
        e = inner;
        target = t;
        ctx = MaskContext::Modular;
        if !elides_mask(e, MaskContext::Modular, limit) {
            break;
        }
    }
    Some((e, ctx, target))
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
    use MaskContext::{Masked, Modular, Reducing};

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

    fn elides_modular(e: &Expr) -> bool {
        elides_mask(e, Modular, LIMIT)
    }

    #[test]
    fn i32_add_and_sub_of_full_range_operands_elide() {
        assert!(elides_modular(&bin(BinOp::I32Add, local(0), local(1))));
        assert!(elides_modular(&bin(BinOp::I32Sub, local(0), local(1))));
    }

    #[test]
    fn i32_mul_of_full_range_operands_keeps_its_mask() {
        // (2^32 - 1)^2 exceeds the Fixnum limit.
        assert!(!elides_modular(&bin(BinOp::I32Mul, local(0), local(1))));
    }

    #[test]
    fn i32_mul_of_narrowed_operands_elides() {
        let byte = |idx| bin(BinOp::I32And, local(idx), Expr::I32Const(0xff));
        assert!(elides_modular(&bin(BinOp::I32Mul, byte(0), byte(1))));
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
        assert!(elides_modular(&bin(
            BinOp::I32Mul,
            byte_of_diff(0, 1),
            byte_of_diff(2, 3)
        )));
    }

    #[test]
    fn i32_shr_s_always_elides() {
        // The raw signed value is within [-2^31, 2^31), well inside the limit.
        assert!(elides_modular(&bin(BinOp::I32ShrS, local(0), local(1))));
    }

    #[test]
    fn shl_elides_only_under_a_small_count_or_operand() {
        // Full range shifted by an unknown count reaches 2^63.
        assert!(!elides_modular(&bin(BinOp::I32Shl, local(0), local(1))));
        // An exact small count keeps the raw value within the limit.
        assert!(elides_modular(&bin(
            BinOp::I32Shl,
            local(0),
            Expr::I32Const(4)
        )));
    }

    #[test]
    fn wrap_elides_only_for_a_narrow_i64() {
        let wrap = |e: Expr| Expr::Un(UnOp::I32WrapI64, Box::new(e));
        // A full-range i64 exceeds the limit, so the wrap's mask stays.
        assert!(!elides_modular(&wrap(local(0))));
        assert!(elides_modular(&wrap(load8u())));
        // The high half extracted by `>> 32` is bounded by 2^32.
        assert!(elides_modular(&wrap(bin(
            BinOp::I64ShrU,
            local(0),
            Expr::I64Const(32)
        ))));
    }

    #[test]
    fn i64_arithmetic_elides_only_with_provably_narrow_operands() {
        assert!(!elides_modular(&bin(BinOp::I64Add, local(0), local(1))));
        assert!(!elides_modular(&bin(BinOp::I64ShrS, local(0), local(1))));
        assert!(elides_modular(&bin(
            BinOp::I64Add,
            load8u(),
            Expr::I64Const(1)
        )));
    }

    #[test]
    fn nested_elision_bounds_compose() {
        // (l0 + l1) + l2 unmasked is below 3 * 2^32.
        let inner = bin(BinOp::I32Add, local(0), local(1));
        assert!(elides_modular(&bin(BinOp::I32Add, inner, local(2))));
        // ((l0 + l1) * l2) unmasked reaches 2^65: the mul keeps its mask even though its operand elides.
        let inner = bin(BinOp::I32Add, local(0), local(1));
        assert!(!elides_modular(&bin(BinOp::I32Mul, inner, local(2))));
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
        let l = local(1);
        assert_eq!(bin_operand_context(BinOp::I32Add, 0, &l, Masked), Modular);
        assert_eq!(bin_operand_context(BinOp::I32ShrU, 0, &l, Modular), Masked);
        assert_eq!(bin_operand_context(BinOp::I32ShrU, 1, &l, Masked), Modular);
        assert_eq!(bin_operand_context(BinOp::I32And, 0, &l, Modular), Modular);
        assert_eq!(bin_operand_context(BinOp::I32And, 0, &l, Masked), Masked);
        assert_eq!(bin_operand_context(BinOp::I32DivU, 0, &l, Modular), Masked);
        assert_eq!(un_operand_context(UnOp::I32WrapI64), Modular);
        assert_eq!(un_operand_context(UnOp::I64ExtendI32U), Masked);
    }

    #[test]
    fn a_constant_and_sibling_makes_the_operand_context_reducing() {
        let c = Expr::I32Const(0xff);
        let c64 = Expr::I64Const(0xff);
        let l = local(1);
        assert_eq!(bin_operand_context(BinOp::I32And, 0, &c, Masked), Reducing);
        assert_eq!(
            bin_operand_context(BinOp::I64And, 0, &c64, Masked),
            Reducing
        );
        // Only AND reduces its other operand; OR and XOR pass the high bits through.
        assert_eq!(bin_operand_context(BinOp::I32Or, 0, &c, Masked), Masked);
        assert_eq!(bin_operand_context(BinOp::I32Xor, 0, &c, Masked), Masked);
        // A reducing consumer weakens to modular through a maskless bitwise operator.
        assert_eq!(bin_operand_context(BinOp::I32Xor, 0, &l, Reducing), Modular);
        assert_eq!(bin_operand_context(BinOp::I32And, 0, &l, Reducing), Modular);
    }

    #[test]
    fn a_reducing_consumer_drops_the_mask_with_no_bound_guard() {
        // A full-range product exceeds the limit, so a modular consumer keeps its mask; a reducing one drops it.
        let mul = bin(BinOp::I32Mul, local(0), local(1));
        assert!(!elides_mask(&mul, Modular, LIMIT));
        assert!(elides_mask(&mul, Reducing, LIMIT));
        let wrap = Expr::Un(UnOp::I32WrapI64, Box::new(local(0)));
        assert!(!elides_mask(&wrap, Modular, LIMIT));
        assert!(elides_mask(&wrap, Reducing, LIMIT));
        // A maskless site has nothing to drop in any context.
        assert!(!elides_mask(&local(0), Reducing, LIMIT));
    }

    #[test]
    fn an_identity_mask_drops_in_every_context() {
        // The high half extracted by `>> 32` sits in [0, 2^32): the wrap is the identity even at an observation point.
        let wrap = Expr::Un(
            UnOp::I32WrapI64,
            Box::new(bin(BinOp::I64ShrU, local(0), Expr::I64Const(32))),
        );
        assert!(elides_mask(&wrap, Masked, LIMIT));
        // A raw interval reaching outside [0, 2^32) is not the identity: the mask stays under a masked consumer.
        let add = bin(BinOp::I32Add, local(0), local(1));
        assert!(!elides_mask(&add, Masked, LIMIT));
        let sub = bin(BinOp::I32Sub, local(0), Expr::I32Const(1));
        assert!(!elides_mask(&sub, Masked, LIMIT));
        // A sum of narrowed operands stays inside the width: identity again.
        let narrow_add = bin(BinOp::I32Add, load8u(), Expr::I32Const(1));
        assert!(elides_mask(&narrow_add, Masked, LIMIT));
    }

    #[test]
    fn and_chains_fold_their_constants() {
        let chain = bin(
            BinOp::I32And,
            bin(BinOp::I32And, local(0), Expr::I32Const(0xff00)),
            Expr::I32Const(0x0ff0),
        );
        let Expr::Bin(_, a, b) = &chain else {
            unreachable!()
        };
        let (e, c) = fold_and_chain(a, b).expect("two constants fold");
        assert!(matches!(e, Expr::LocalGet(0)));
        assert_eq!(c, 0x0f00);
        // Constants on either side of either AND fold the same.
        let chain = bin(
            BinOp::I64And,
            Expr::I64Const(0xff),
            bin(BinOp::I64And, Expr::I64Const(0x0f), local(0)),
        );
        let Expr::Bin(_, a, b) = &chain else {
            unreachable!()
        };
        let (e, c) = fold_and_chain(a, b).expect("two constants fold");
        assert!(matches!(e, Expr::LocalGet(0)));
        assert_eq!(c, 0x0f);
    }

    #[test]
    fn and_chain_folding_needs_two_constants() {
        // A single constant is the plain AND shape, not a chain.
        assert!(fold_and_chain(&local(0), &Expr::I32Const(0xff)).is_none());
        // No constant at the outer AND leaves nothing to fold into.
        let inner = bin(BinOp::I32And, local(0), Expr::I32Const(0xff));
        assert!(fold_and_chain(&inner, &local(1)).is_none());
    }

    #[test]
    fn eq_rewrite_pins_a_unique_preimage_and_migrates_the_constant() {
        // `(l0 - 5) & 0xffffffff == 7` has raw interval [-5, 2^32 - 6]: only 7 fits, and the constant migrates to `l0 == 12`.
        let sub = bin(BinOp::I32Sub, local(0), Expr::I32Const(5));
        let (e, ctx, t) = eq_const_rewrite(&sub, 7, LIMIT).expect("unique preimage");
        assert!(matches!(e, Expr::LocalGet(0)));
        assert_eq!(ctx, Modular);
        assert_eq!(t, 12);
        // `5 - l0` flips the migration: `l0 == 5 - 7` lands on the wrapped candidate 7 - 2^32.
        let rsub = bin(BinOp::I32Sub, Expr::I32Const(5), local(0));
        let (e, _, t) = eq_const_rewrite(&rsub, 7, LIMIT).expect("unique preimage");
        assert!(matches!(e, Expr::LocalGet(0)));
        assert_eq!(t, 5 - (7 - (1i128 << 32)));
        // Migration walks through nested elided layers: `(l0 + 3) - 5 == 7` pins `l0 == 9`.
        let nested = bin(
            BinOp::I32Sub,
            bin(BinOp::I32Add, local(0), Expr::I32Const(3)),
            Expr::I32Const(5),
        );
        let (e, _, t) = eq_const_rewrite(&nested, 7, LIMIT).expect("unique preimage");
        assert!(matches!(e, Expr::LocalGet(0)));
        assert_eq!(t, 9);
        // The wrap peels away like an add/sub layer, leaving its operand in modular context.
        let wrap = Expr::Un(UnOp::I32WrapI64, Box::new(load8u()));
        let (e, ctx, t) = eq_const_rewrite(&wrap, 7, LIMIT).expect("unique preimage");
        assert!(matches!(e, Expr::Load { .. }));
        assert_eq!(ctx, Modular);
        assert_eq!(t, 7);
        // A site with no peelable layer compares its whole raw form in reducing context: `i64.shr_s` raw is within [-2^63, 2^63), so only the candidate 7 fits.
        let shr = bin(BinOp::I64ShrS, local(0), local(1));
        let (e, ctx, t) = eq_const_rewrite(&shr, 7, LIMIT).expect("unique preimage");
        assert!(matches!(e, Expr::Bin(BinOp::I64ShrS, ..)));
        assert_eq!(ctx, Reducing);
        assert_eq!(t, 7);
    }

    #[test]
    fn eq_rewrite_against_zero_pins_a_two_sided_sub() {
        // The sub's open interval (-2^32, 2^32) holds exactly one multiple of 2^32: zero itself, so `eqz(x - y)` reads the raw difference.
        let sub = bin(BinOp::I32Sub, local(0), local(1));
        let (e, ctx, t) = eq_const_rewrite(&sub, 0, LIMIT).expect("unique preimage");
        assert!(matches!(e, Expr::Bin(BinOp::I32Sub, ..)));
        assert_eq!(ctx, Reducing);
        assert_eq!(t, 0);
    }

    #[test]
    fn eq_rewrite_keeps_the_mask_without_a_unique_preimage() {
        // A two-sided sub spans almost 2^33: both 7 and 7 - 2^32 fit.
        let sub = bin(BinOp::I32Sub, local(0), local(1));
        assert!(eq_const_rewrite(&sub, 7, LIMIT).is_none());
        // A narrow interval missing the constant fits no candidate: statically false, but the operand must still run, so the mask stays.
        let narrow = bin(BinOp::I32Add, load8u(), Expr::I32Const(1));
        assert!(eq_const_rewrite(&narrow, 500, LIMIT).is_none());
        // A maskless expression is no rewrite site.
        assert!(eq_const_rewrite(&local(0), 7, LIMIT).is_none());
    }
}
