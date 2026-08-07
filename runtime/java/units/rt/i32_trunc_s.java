// requires: rt/trap
// i32.trunc_f32_s / i32.trunc_f64_s: trapping signed truncation. Java's `(int)`
// cast saturates, so wasm's two trap conditions (NaN, out-of-range) need
// explicit checks. The source is always widened to double first
// (exact for f32), so one helper serves both widths.
static int i32_trunc_s(double x) {
    if (Double.isNaN(x)) {
        trap("invalid conversion to integer");
    }
    if (Double.isInfinite(x)) {
        trap("integer overflow");
    }
    double t = x < 0 ? Math.ceil(x) : Math.floor(x);
    if (t < -2147483648.0 || t > 2147483647.0) {
        trap("integer overflow");
    }
    return (int) t;
}
