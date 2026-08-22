// requires: rt/trap
// i64.trunc_f32_u / i64.trunc_f64_u: trapping unsigned truncation.
// The result is the u64 value carried as a signed `long` bit pattern; values in
// [2^63, 2^64) map to negative longs.
// Java's cast saturates at 2^63, so the high half is reconstructed by subtracting 2^63 and setting the sign bit.
static long i64_trunc_u(double x) {
    if (Double.isNaN(x)) {
        trap("invalid conversion to integer");
    }
    if (Double.isInfinite(x)) {
        trap("integer overflow");
    }
    double t = x < 0 ? Math.ceil(x) : Math.floor(x);
    if (t < 0 || t >= 18446744073709551616.0) {
        trap("integer overflow");
    }
    if (t >= 9223372036854775808.0) {
        return (long) (t - 9223372036854775808.0) | Long.MIN_VALUE;
    }
    return (long) t;
}
