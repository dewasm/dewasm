// f64.promote_f32: a NaN keeps its sign and its payload is shifted into the
// wider significand (arithmetic result); finite values promote exactly.
static double f64_promote(float x) {
    if (Float.isNaN(x)) {
        int b = Float.floatToRawIntBits(x);
        return Double.longBitsToDouble(
            ((long) (b >>> 31) << 63) | 0x7ff8000000000000L | ((long) (b & 0x7fffff) << 29));
    }
    return (double) x;
}
