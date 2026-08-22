// f32.demote_f64: a NaN keeps its sign and takes an arithmetic (canonical)
// payload; finite values round to nearest-even via the JVM d2f.
static float f32_demote(double x) {
    if (Double.isNaN(x)) {
        long b = Double.doubleToRawLongBits(x);
        return Float.intBitsToFloat((int) (b >>> 32) & 0x80000000 | 0x7fc00000);
    }
    return (float) x;
}
