// i32.trunc_sat_f32_u / i32.trunc_sat_f64_u: saturating unsigned truncation.
// Java's `(int)(long)` would wrap past u32, so saturation is explicit (NaN->0, clamp to [0, 2^32)); the signed saturating forms use a plain `(int)` cast, which already matches wasm.
static int i32_trunc_sat_u(double x) {
    if (Double.isNaN(x)) {
        return 0;
    }
    double t = x < 0 ? Math.ceil(x) : Math.floor(x);
    if (t <= 0) {
        return 0;
    }
    if (t >= 4294967295.0) {
        return -1;
    }
    return (int) (long) t;
}
