// f64 counterpart of f32_canon (ADR-2).
static double f64_canon(double x) {
    return Double.isNaN(x) ? Double.longBitsToDouble(0x7ff8000000000000L) : x;
}
