// requires: rt/trap
// i32.trunc_f32_u / i32.trunc_f64_u: trapping unsigned truncation.
// The result is the u32 value carried in the low 32 bits of an `int` (e.g. 0xffffffff for
// 4294967295).
// Java's cast saturates, so NaN/out-of-range are checked.
static int i32_trunc_u(double x) {
    if (Double.isNaN(x)) {
        trap("invalid conversion to integer");
    }
    if (Double.isInfinite(x)) {
        trap("integer overflow");
    }
    double t = x < 0 ? Math.ceil(x) : Math.floor(x);
    if (t < 0 || t > 4294967295.0) {
        trap("integer overflow");
    }
    return (int) (long) t;
}
