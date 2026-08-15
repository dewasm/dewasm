//! Mask elision for the backends that store integers masked-unsigned.
//!
//! The stored representation masks every integer to its width, and each wrapping operation re-masks its result.
//! That is often double work: a consumer that reads its operand only modulo 2^32 or 2^64 (a wrapping add's operand, a shift count) cannot observe the high bits its own mask throws away.
//! [`MaskContext`] names the two consumption contexts, [`bin_operand_context`] and [`un_operand_context`] are the table of which operands an operation reads modularly, and the guard is an interval bound: a mask is skipped only when the value it would expose provably stays within the backend's unboxed-integer range.
//! [`elides_mask`] applies this inside one expression tree; [`Elision`] extends it across statements with a per-function dataflow that lets a local or temp store an unmasked value when every read of it is modular.
//!
//! Soundness rests on the targets' integers being arbitrary-precision two's complement (Ruby, Python, Perl): an unmasked value is congruent to the masked value modulo 2^w, every modular consumer preserves that congruence (a bitwise operator reads a negative operand as its infinite two's-complement form, so `(x - y) & 0xffffffff` is the correct wrap of a negative difference), and every non-modular consumer either sits behind a kept mask or reads a variable all of whose stores are masked.

use std::collections::BTreeMap;

use dewasm_core::ir::{BinOp, BrTarget, Expr, Func, LoadOp, Stmt, Temp, UnOp, ValType};

/// How a consumer reads an integer operand: `Masked` when it observes the exact stored value, `Modular` when it reads only the value's congruence class modulo the type width, so an unmasked operand serves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskContext {
    Masked,
    Modular,
}

/// The context in which `op` consumes its operand `k` (0-based), given that `op`'s own result is consumed in `ctx`.
///
/// An operation with its own result mask (wrapping add/sub/mul, `shl`, the wrap) reads its operands modularly regardless of `ctx`: the site's mask restores the invariant even when its own elision guard fails.
/// A shift count is read through the semantic `& (w - 1)`, so it is modular for every shift.
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

/// [`Elision::elides_mask`] with no variables tracked: every local and temp read counts as its full masked width, so the guard holds only from the expression tree itself.
pub fn elides_mask(e: &Expr, limit: i128) -> bool {
    Elision::none(limit).elides_mask(e)
}

/// Everything the interval analysis tracks is clamped to this magnitude: far beyond any elision limit, and small enough that saturating i128 arithmetic on two clamped bounds cannot wrap.
const CEILING: i128 = 1 << 100;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

/// Which of one function's locals and temps may store an unmasked value, each with the interval its stored values stay in.
///
/// A variable qualifies when the analysis proves two things.
/// Every read of it is modular: an operand position the consumption table clears, or a copy into another qualifying variable.
/// A comparison, a division, a signed or unsigned view, an address, a call argument, a return, a store, a global set, or any position the model does not cover disqualifies, which keeps the function boundary (decision 2's ABI) and every helper call fully masked; a function parameter is defined by the caller at full masked width.
/// And its interval converges within `limit`: each definition's bound (the expression bound with qualifying variables read at their current intervals, an external definition such as a call result at full masked width) is joined to a fixpoint, so a loop-carried definition whose unmasked updates compound past the limit is demoted and keeps its masks.
pub struct Elision {
    limit: i128,
    locals: BTreeMap<u32, Bound>,
    temps: BTreeMap<Temp, Bound>,
}

impl Elision {
    /// The empty analysis: no variable is tracked, so only within-tree elision applies.
    pub fn none(limit: i128) -> Elision {
        Elision {
            limit,
            locals: BTreeMap::new(),
            temps: BTreeMap::new(),
        }
    }

    /// Run the dataflow over `func` (`params` are the function's parameter types, indexed below its declared locals).
    pub fn analyze(params: &[ValType], func: &Func, limit: i128) -> Elision {
        let mut el = Elision::none(limit);
        for (i, ty) in params.iter().chain(&func.locals).enumerate() {
            if int_bits(*ty).is_some() {
                el.locals.insert(i as u32, Bound::exact(0));
            }
        }
        for t in &func.temps {
            if int_bits(t.ty).is_some() {
                el.temps.insert(*t, Bound::exact(0));
            }
        }
        // A demotion turns a variable's stores back into masked observation roots, which can disqualify further variables, so both halves rerun until neither changes anything.
        loop {
            qualify(&mut el, func);
            if solve(&mut el, params, func) == 0 {
                break;
            }
        }
        el
    }

    /// Whether stores to local `idx` may render their value in modular context, with no store mask of their own.
    pub fn unmasked_local(&self, idx: u32) -> bool {
        self.locals.contains_key(&idx)
    }

    /// The [`Elision::unmasked_local`] counterpart for temps.
    pub fn unmasked_temp(&self, t: Temp) -> bool {
        self.temps.contains_key(&t)
    }

    /// Whether `e`'s own result mask may be skipped when its consumer is modular: `e` must carry a mask of its own, and the value it exposes unmasked must provably stay in `[-limit, limit)`, with tracked variables read at their analyzed intervals.
    /// `limit` is the backend's unboxed-integer bound (Ruby: `1 << 62`); the guard keeps elision strictly profitable by never exposing an intermediate the mask would have kept out of bignum arithmetic.
    pub fn elides_mask(&self, e: &Expr) -> bool {
        match self.raw_bound(e) {
            Some(raw) => raw.within(self.limit),
            None => false,
        }
    }

    fn member(&self, var: Var) -> bool {
        match var {
            Var::Local(i) => self.locals.contains_key(&i),
            Var::Temp(t) => self.temps.contains_key(&t),
        }
    }

    /// Join a definition's bound into `var`'s interval, reporting whether it changed.
    /// From pass [`WIDEN_AFTER`] on, a still-changing interval is widened, to its masked width first and past the limit after that: growth too slow to wait out (a loop counter gaining 1 per pass) ends in a bounded number of steps, while a definition whose bound is narrow on its own (a bitwise `&`) still settles at the masked width instead of being demoted.
    fn join(&mut self, var: Var, b: Bound, bits: u32, pass: usize) -> bool {
        let cur = match var {
            Var::Local(i) => self.locals.get_mut(&i),
            Var::Temp(t) => self.temps.get_mut(&t),
        };
        let Some(cur) = cur else { return false };
        let mut next = cur.union(b);
        if next == *cur {
            return false;
        }
        if pass >= WIDEN_AFTER {
            next = if cur.lo <= 0 && cur.hi >= (1i128 << bits) - 1 {
                Bound::new(-CEILING, CEILING)
            } else {
                next.union(Bound::masked(bits))
            };
        }
        *cur = next;
        true
    }

    /// The interval of the value `e`'s rendering takes when consumed in `ctx`, under the elision policy of [`Elision::elides_mask`].
    /// `bits` is the width of `e`'s integer type, which the caller knows from the operation consuming it.
    fn bound(&self, e: &Expr, bits: u32, ctx: MaskContext) -> Bound {
        if let Some(raw) = self.raw_bound(e) {
            if ctx == MaskContext::Modular && raw.within(self.limit) {
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
            Expr::LocalGet(i) => self
                .locals
                .get(i)
                .copied()
                .unwrap_or_else(|| Bound::masked(bits)),
            Expr::Temp(t) => self
                .temps
                .get(t)
                .copied()
                .unwrap_or_else(|| Bound::masked(bits)),
            Expr::GlobalGet(_) => Bound::masked(bits),
            // wasm's memory is capped at 65536 pages.
            Expr::MemorySize => Bound::new(0, 1 << 16),
            Expr::Load { op, .. } => load_bound(*op, bits),
            Expr::Select { then, els, .. } => self
                .bound(then, bits, Masked)
                .union(self.bound(els, bits, Masked)),
            Expr::Un(op, a) => match op {
                UnOp::I32Eqz | UnOp::I64Eqz => Bound::new(0, 1),
                UnOp::I32Clz | UnOp::I32Ctz | UnOp::I32Popcnt => Bound::new(0, 32),
                UnOp::I64Clz | UnOp::I64Ctz | UnOp::I64Popcnt => Bound::new(0, 64),
                UnOp::I64ExtendI32U => self.bound(a, 32, Masked),
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
                        self.bound(a, bits, ctx),
                        self.bound(b, bits, ctx),
                    ),
                    I32ShrU | I64ShrU => {
                        let ba = self.bound(a, bits, Masked);
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
    fn raw_bound(&self, e: &Expr) -> Option<Bound> {
        use BinOp::*;
        use MaskContext::Modular;
        match e {
            Expr::Un(UnOp::I32WrapI64, a) => Some(self.bound(a, 64, Modular)),
            Expr::Bin(op, a, b) => {
                let bits = match op {
                    I32Add | I32Sub | I32Mul | I32Shl | I32ShrS => 32,
                    I64Add | I64Sub | I64Mul | I64Shl | I64ShrS => 64,
                    _ => return None,
                };
                let ba = self.bound(a, bits, bin_operand_context(*op, 0, Modular));
                let bb = |b: &Expr| self.bound(b, bits, Modular);
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
}

/// The integer width the masking convention covers; floats and references are never mask candidates.
fn int_bits(ty: ValType) -> Option<u32> {
    match ty {
        ValType::I32 => Some(32),
        ValType::I64 => Some(64),
        _ => None,
    }
}

/// A store's destination: the two variable kinds the dataflow tracks.
#[derive(Clone, Copy)]
enum Var {
    Local(u32),
    Temp(Temp),
}

/// One dataflow-relevant position directly in a statement.
enum Site<'a> {
    /// `var = expr` (`Stmt::LocalSet` / `Stmt::Assign`): rendered in modular context when `var` stores unmasked.
    Assign(Var, &'a Expr),
    /// `dst = src`, a branch-result copy between temps (`BrTarget::Label`'s `assigns`).
    Copy(Temp, Temp),
    /// `temp` receives an externally produced value of full masked width: a call result, `memory.grow`, an exception payload.
    External(Temp),
    /// `expr` is a statement root whose exact value is observed; reads inside it follow the consumption table from `Masked` down.
    Observe(&'a Expr),
}

/// Every dataflow-relevant position directly in `stmt`; nested statement sequences are reached by [`Stmt::any`].
/// A condition is an observation root: boolean lowering tests the exact value (comparison operands, zero tests), and the table takes over below it.
fn sites<'a>(stmt: &'a Stmt, f: &mut impl FnMut(Site<'a>)) {
    fn target<'a>(t: &'a BrTarget, f: &mut dyn FnMut(Site<'a>)) {
        match t {
            BrTarget::Return { values } => {
                for v in values {
                    f(Site::Observe(v));
                }
            }
            BrTarget::Label { assigns, .. } => {
                for (dst, src) in assigns {
                    f(Site::Copy(*dst, *src));
                }
            }
        }
    }
    match stmt {
        Stmt::Assign { dst, expr } => f(Site::Assign(Var::Temp(*dst), expr)),
        Stmt::LocalSet { idx, expr } => f(Site::Assign(Var::Local(*idx), expr)),
        Stmt::GlobalSet { expr, .. } => f(Site::Observe(expr)),
        Stmt::Store { addr, value, .. } => {
            f(Site::Observe(addr));
            f(Site::Observe(value));
        }
        Stmt::If { cond, .. } => f(Site::Observe(cond)),
        Stmt::Br(t) => target(t, f),
        Stmt::BrIf { cond, target: t } => {
            f(Site::Observe(cond));
            target(t, f);
        }
        Stmt::BrTable {
            index,
            targets,
            default,
        } => {
            f(Site::Observe(index));
            for t in targets {
                target(t, f);
            }
            target(default, f);
        }
        Stmt::Return { values } => {
            for v in values {
                f(Site::Observe(v));
            }
        }
        Stmt::Call { args, results, .. } => {
            for a in args {
                f(Site::Observe(a));
            }
            for r in results {
                f(Site::External(*r));
            }
        }
        Stmt::CallIndirect {
            index,
            args,
            results,
            ..
        } => {
            f(Site::Observe(index));
            for a in args {
                f(Site::Observe(a));
            }
            for r in results {
                f(Site::External(*r));
            }
        }
        Stmt::MemoryGrow { dst, delta } => {
            f(Site::Observe(delta));
            f(Site::External(*dst));
        }
        Stmt::MemoryCopy { dst, src, len }
        | Stmt::MemoryInit { dst, src, len, .. }
        | Stmt::TableInit { dst, src, len, .. }
        | Stmt::TableCopy { dst, src, len, .. } => {
            f(Site::Observe(dst));
            f(Site::Observe(src));
            f(Site::Observe(len));
        }
        Stmt::MemoryFill { dst, val, len } => {
            f(Site::Observe(dst));
            f(Site::Observe(val));
            f(Site::Observe(len));
        }
        Stmt::TryTable { catches, .. } => {
            for c in catches {
                for t in &c.value_temps {
                    f(Site::External(*t));
                }
                target(&c.target, f);
            }
        }
        Stmt::Throw { args, .. } => {
            for a in args {
                f(Site::Observe(a));
            }
        }
        Stmt::ThrowRef { exn } => f(Site::Observe(exn)),
        Stmt::SourceLine(_)
        | Stmt::Block { .. }
        | Stmt::Loop { .. }
        | Stmt::DataDrop { .. }
        | Stmt::ElemDrop { .. }
        | Stmt::Unreachable => {}
    }
}

/// The reads of locals and temps inside `e` that observe the exact value, mirroring the contexts emission threads through the tree: operands per the consumption table, a load address, `Select`'s condition and arms.
/// One refinement beyond the table: `&` with a constant operand observes only the bits that constant keeps, all within the width (constants are stored masked), so the other operand is modular whatever the consumer; only the read classification uses this, emission contexts are unchanged.
fn scan_reads(e: &Expr, ctx: MaskContext, on_masked_read: &mut impl FnMut(&Expr)) {
    match e {
        Expr::LocalGet(_) | Expr::Temp(_) => {
            if ctx == MaskContext::Masked {
                on_masked_read(e);
            }
        }
        Expr::Un(op, a) => scan_reads(a, un_operand_context(*op), on_masked_read),
        Expr::Bin(op, a, b) => {
            let is_const = |e: &Expr| matches!(e, Expr::I32Const(_) | Expr::I64Const(_));
            let refine = |sibling: &Expr, table: MaskContext| {
                if matches!(op, BinOp::I32And | BinOp::I64And) && is_const(sibling) {
                    MaskContext::Modular
                } else {
                    table
                }
            };
            scan_reads(
                a,
                refine(b, bin_operand_context(*op, 0, ctx)),
                on_masked_read,
            );
            scan_reads(
                b,
                refine(a, bin_operand_context(*op, 1, ctx)),
                on_masked_read,
            );
        }
        Expr::Load { addr, .. } => scan_reads(addr, MaskContext::Masked, on_masked_read),
        Expr::Select { cond, then, els } => {
            scan_reads(cond, MaskContext::Masked, on_masked_read);
            scan_reads(then, MaskContext::Masked, on_masked_read);
            scan_reads(els, MaskContext::Masked, on_masked_read);
        }
        Expr::I32Const(_)
        | Expr::I64Const(_)
        | Expr::F32Const(_)
        | Expr::F64Const(_)
        | Expr::GlobalGet(_)
        | Expr::MemorySize => {}
    }
}

fn remove_read(el: &mut Elision, read: &Expr) -> bool {
    match read {
        Expr::LocalGet(i) => el.locals.remove(i).is_some(),
        Expr::Temp(t) => el.temps.remove(t).is_some(),
        _ => false,
    }
}

/// The qualification half: remove every variable one of whose reads observes the exact value.
/// It starts from everything tracked and only removes; a store root or a copy counts as modular exactly while its destination is still tracked, so removals cascade, and each changing pass removes at least one variable, which bounds the passes.
/// The surviving set is consistent as a whole: copy cycles among survivors are sound because no survivor is ever observed exactly.
fn qualify(el: &mut Elision, func: &Func) {
    loop {
        let mut changed = false;
        Stmt::any(&func.body, &mut |stmt| {
            sites(stmt, &mut |site| match site {
                Site::Assign(var, e) => {
                    let ctx = if el.member(var) {
                        MaskContext::Modular
                    } else {
                        MaskContext::Masked
                    };
                    scan_reads(e, ctx, &mut |read| changed |= remove_read(el, read));
                }
                Site::Copy(dst, src) => {
                    if !el.member(Var::Temp(dst)) {
                        changed |= el.temps.remove(&src).is_some();
                    }
                }
                Site::External(_) => {}
                Site::Observe(e) => {
                    scan_reads(e, MaskContext::Masked, &mut |read| {
                        changed |= remove_read(el, read)
                    });
                }
            });
            false
        });
        if !changed {
            break;
        }
    }
}

/// Kleene passes before [`Elision::join`] starts widening: enough for chains of narrow definitions to settle without it.
const WIDEN_AFTER: usize = 3;

/// The interval half: Kleene iteration from the seeds (a parameter at full masked width, a declared local at its zero initializer, a temp at zero), joining every definition's bound until a pass changes nothing.
/// Joins only grow and widening caps how often each variable can change, so the iteration ends; the unchanged pass certifies that every tracked interval contains all of its definitions' bounds, which is what makes the recorded intervals hold at runtime.
/// Variables whose interval breached the limit are then demoted to must-mask; the count of demotions is returned.
fn solve(el: &mut Elision, params: &[ValType], func: &Func) -> usize {
    for (i, b) in el.locals.iter_mut() {
        let i = *i as usize;
        *b = if i < params.len() {
            Bound::masked(int_bits(params[i]).expect("tracked locals are integers"))
        } else {
            Bound::exact(0)
        };
    }
    for b in el.temps.values_mut() {
        *b = Bound::exact(0);
    }
    let mut pass = 0;
    loop {
        let mut changed = false;
        Stmt::any(&func.body, &mut |stmt| {
            sites(stmt, &mut |site| match site {
                Site::Assign(var, e) => {
                    if el.member(var) {
                        let bits = var_bits(var, params, func).expect("tracked vars are integers");
                        let b = el.bound(e, bits, MaskContext::Modular);
                        changed |= el.join(var, b, bits, pass);
                    }
                }
                Site::Copy(dst, src) => {
                    if let Some(bits) = int_bits(src.ty) {
                        let b = el
                            .temps
                            .get(&src)
                            .copied()
                            .unwrap_or_else(|| Bound::masked(bits));
                        changed |= el.join(Var::Temp(dst), b, bits, pass);
                    }
                }
                Site::External(t) => {
                    if let Some(bits) = int_bits(t.ty) {
                        changed |= el.join(Var::Temp(t), Bound::masked(bits), bits, pass);
                    }
                }
                Site::Observe(_) => {}
            });
            false
        });
        if !changed {
            break;
        }
        pass += 1;
    }
    let limit = el.limit;
    let mut demoted = 0;
    let mut keep = |b: &Bound| {
        let keep = b.within(limit);
        if !keep {
            demoted += 1;
        }
        keep
    };
    el.locals.retain(|_, b| keep(b));
    el.temps.retain(|_, b| keep(b));
    demoted
}

fn var_bits(var: Var, params: &[ValType], func: &Func) -> Option<u32> {
    match var {
        Var::Local(i) => {
            let i = i as usize;
            let ty = if i < params.len() {
                params[i]
            } else {
                func.locals[i - params.len()]
            };
            int_bits(ty)
        }
        Var::Temp(t) => int_bits(t.ty),
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

#[cfg(test)]
mod dataflow {
    use super::*;
    use dewasm_core::ir::Label;

    const LIMIT: i128 = 1 << 62;

    fn local(idx: u32) -> Expr {
        Expr::LocalGet(idx)
    }

    fn c32(v: u32) -> Expr {
        Expr::I32Const(v)
    }

    fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
        Expr::Bin(op, Box::new(a), Box::new(b))
    }

    fn func(locals: Vec<ValType>, temps: Vec<Temp>, body: Vec<Stmt>) -> Func {
        Func {
            type_idx: 0,
            locals,
            temps,
            body,
        }
    }

    fn t32(depth: u32) -> Temp {
        Temp {
            depth,
            ty: ValType::I32,
        }
    }

    #[test]
    fn a_local_with_only_modular_reads_stores_unmasked() {
        let f = func(
            vec![ValType::I32],
            vec![],
            vec![
                Stmt::LocalSet {
                    idx: 1,
                    expr: bin(BinOp::I32Add, local(0), c32(5)),
                },
                Stmt::Return {
                    values: vec![bin(BinOp::I32Add, local(1), c32(1))],
                },
            ],
        );
        let el = Elision::analyze(&[ValType::I32], &f, LIMIT);
        assert!(el.unmasked_local(1));
    }

    #[test]
    fn an_exactly_observed_read_disqualifies_the_local() {
        // Returning the local directly observes its exact stored value: the ABI needs the mask.
        let f = func(
            vec![ValType::I32],
            vec![],
            vec![
                Stmt::LocalSet {
                    idx: 1,
                    expr: bin(BinOp::I32Add, local(0), c32(5)),
                },
                Stmt::Return {
                    values: vec![local(1)],
                },
            ],
        );
        let el = Elision::analyze(&[ValType::I32], &f, LIMIT);
        assert!(!el.unmasked_local(1));
    }

    fn counting_loop(update: Expr) -> Func {
        func(
            vec![ValType::I32],
            vec![],
            vec![
                Stmt::Loop {
                    label: Label {
                        id: 0,
                        referenced: true,
                    },
                    body: vec![
                        Stmt::LocalSet {
                            idx: 1,
                            expr: update,
                        },
                        Stmt::BrIf {
                            cond: local(0),
                            target: BrTarget::Label {
                                label: 0,
                                is_loop: true,
                                assigns: vec![],
                            },
                        },
                    ],
                },
                Stmt::Return {
                    values: vec![bin(BinOp::I32Add, local(1), local(0))],
                },
            ],
        )
    }

    #[test]
    fn a_loop_carried_definition_narrowed_by_and_converges() {
        // l1 = (l1 + 1) & 255 in a loop: the `&` re-narrows every iteration, so the interval settles and the store elides.
        let update = bin(
            BinOp::I32And,
            bin(BinOp::I32Add, local(1), c32(1)),
            c32(255),
        );
        let el = Elision::analyze(&[ValType::I32], &counting_loop(update), LIMIT);
        assert!(el.unmasked_local(1));
    }

    #[test]
    fn a_compounding_loop_carried_definition_is_demoted() {
        // l1 = l1 + 1 in a loop: the unmasked interval grows without bound, so widening pushes it past the limit and the store keeps its mask.
        let update = bin(BinOp::I32Add, local(1), c32(1));
        let el = Elision::analyze(&[ValType::I32], &counting_loop(update), LIMIT);
        assert!(!el.unmasked_local(1));
    }

    fn copy_pair(read_of_dst: Expr) -> Func {
        func(
            vec![],
            vec![t32(0), t32(1)],
            vec![
                Stmt::Assign {
                    dst: t32(0),
                    expr: bin(BinOp::I32Add, local(0), c32(1)),
                },
                Stmt::Br(BrTarget::Label {
                    label: 0,
                    is_loop: false,
                    assigns: vec![(t32(1), t32(0))],
                }),
                Stmt::Return {
                    values: vec![read_of_dst],
                },
            ],
        )
    }

    #[test]
    fn a_bit_test_under_a_constant_mask_reads_modularly() {
        // `l1 & 1` in a condition observes only bit 0, which congruence preserves, so it does not disqualify l1.
        let f = func(
            vec![ValType::I32],
            vec![],
            vec![
                Stmt::LocalSet {
                    idx: 1,
                    expr: bin(BinOp::I32Add, local(0), c32(5)),
                },
                Stmt::If {
                    label: Label {
                        id: 0,
                        referenced: false,
                    },
                    cond: bin(BinOp::I32And, local(1), c32(1)),
                    then: vec![Stmt::Return {
                        values: vec![c32(0)],
                    }],
                    els: vec![],
                },
                Stmt::Return {
                    values: vec![bin(BinOp::I32Add, local(1), c32(1))],
                },
            ],
        );
        let el = Elision::analyze(&[ValType::I32], &f, LIMIT);
        assert!(el.unmasked_local(1));
    }

    #[test]
    fn a_copy_into_an_observed_temp_disqualifies_the_source() {
        let f = copy_pair(Expr::Temp(t32(1)));
        let el = Elision::analyze(&[ValType::I32], &f, LIMIT);
        assert!(!el.unmasked_temp(t32(1)));
        assert!(!el.unmasked_temp(t32(0)));
    }

    #[test]
    fn a_copy_between_qualifying_temps_stays_unmasked() {
        let f = copy_pair(bin(BinOp::I32Add, Expr::Temp(t32(1)), c32(1)));
        let el = Elision::analyze(&[ValType::I32], &f, LIMIT);
        assert!(el.unmasked_temp(t32(0)));
        assert!(el.unmasked_temp(t32(1)));
    }

    #[test]
    fn an_i64_call_result_breaches_the_limit_and_keeps_masks() {
        let t64 = Temp {
            depth: 0,
            ty: ValType::I64,
        };
        let f = func(
            vec![],
            vec![t64],
            vec![
                Stmt::Call {
                    func: 0,
                    args: vec![],
                    results: vec![t64],
                },
                Stmt::Return {
                    values: vec![bin(BinOp::I64Add, Expr::Temp(t64), Expr::I64Const(1))],
                },
            ],
        );
        let el = Elision::analyze(&[], &f, LIMIT);
        // The full masked i64 width reaches 2^64, past the Fixnum limit, so even a purely modular consumer set cannot clear it.
        assert!(!el.unmasked_temp(t64));
    }

    #[test]
    fn a_tracked_interval_refines_the_expression_guard() {
        // l1 is confined to a byte by its only definition, so a product of two reads elides where masked-width operands would not.
        let f = func(
            vec![ValType::I32],
            vec![],
            vec![
                Stmt::LocalSet {
                    idx: 1,
                    expr: bin(BinOp::I32And, local(0), c32(255)),
                },
                Stmt::Return {
                    values: vec![bin(
                        BinOp::I32Add,
                        bin(BinOp::I32Mul, local(1), local(1)),
                        c32(1),
                    )],
                },
            ],
        );
        let el = Elision::analyze(&[ValType::I32], &f, LIMIT);
        assert!(el.unmasked_local(1));
        let product = bin(BinOp::I32Mul, local(1), local(1));
        assert!(el.elides_mask(&product));
        assert!(!Elision::none(LIMIT).elides_mask(&product));
    }
}
