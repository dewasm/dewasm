// f64.min (see f32_min).
static double f64_min(double a, double b) {
    if (Double.isNaN(a) || Double.isNaN(b)) {
        return Double.longBitsToDouble(0x7ff8000000000000L);
    }
    if (a < b) {
        return a;
    }
    if (b < a) {
        return b;
    }
    if (a == 0.0) {
        return (Double.doubleToRawLongBits(a) | Double.doubleToRawLongBits(b)) < 0L ? -0.0 : 0.0;
    }
    return a;
}
