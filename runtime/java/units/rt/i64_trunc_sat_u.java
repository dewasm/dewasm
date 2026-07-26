// i64.trunc_sat_f32_u / i64.trunc_sat_f64_u: saturating unsigned truncation.
// Result is a u64 value carried as a signed `long` bit pattern (see
// i64_trunc_u); NaN->0, clamp to [0, 2^64) (ADR-2).
static long i64_trunc_sat_u(double x) {
    if (Double.isNaN(x)) {
        return 0L;
    }
    double t = x < 0 ? Math.ceil(x) : Math.floor(x);
    if (t <= 0) {
        return 0L;
    }
    if (t >= 18446744073709551616.0) {
        return -1L;
    }
    if (t >= 9223372036854775808.0) {
        return (long) (t - 9223372036854775808.0) | Long.MIN_VALUE;
    }
    return (long) t;
}
