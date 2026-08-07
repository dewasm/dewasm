// f32.convert_i64_u: unsigned i64 (a signed `long` bit pattern) to f32. Java's
// l2f is signed, so values >= 2^63 (negative longs) need the round-to-odd
// trick: `(x >>> 1) | (x & 1)` halves the value while making the shifted-out
// bit sticky, so the single l2f rounding that follows is correctly
// round-to-nearest-even, with no double rounding.
static float f32_convert_i64_u(long x) {
    if (x >= 0) {
        return (float) x;
    }
    return (float) ((x >>> 1) | (x & 1L)) * 2.0f;
}
