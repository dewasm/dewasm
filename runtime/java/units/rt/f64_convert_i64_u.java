// f64.convert_i64_u: unsigned i64 (a signed `long` bit pattern) to f64, via the
// round-to-odd trick (see f32_convert_i64_u) so values >= 2^63 round correctly
// under Java's signed l2d (ADR-2).
static double f64_convert_i64_u(long x) {
    if (x >= 0) {
        return (double) x;
    }
    return (double) ((x >>> 1) | (x & 1L)) * 2.0;
}
