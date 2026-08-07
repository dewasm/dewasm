// requires: rt/trap
// i64.trunc_f32_s / i64.trunc_f64_s: trapping signed truncation (see
// i32_trunc_s). The upper bound is `>= 2^63` since 2^63 is not representable as
// i64.
static long i64_trunc_s(double x) {
    if (Double.isNaN(x)) {
        trap("invalid conversion to integer");
    }
    if (Double.isInfinite(x)) {
        trap("integer overflow");
    }
    double t = x < 0 ? Math.ceil(x) : Math.floor(x);
    if (t < -9223372036854775808.0 || t >= 9223372036854775808.0) {
        trap("integer overflow");
    }
    return (long) t;
}
