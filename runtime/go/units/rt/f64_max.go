func (rt) f64_max(a, b float64) float64 {
    // A NaN operand yields the wasm canonical NaN (see f64_min); Go's
    // math.NaN() is not bit-canonical.
    if math.IsNaN(a) || math.IsNaN(b) {
        return math.Float64frombits(0x7ff8000000000000)
    }
    if a > b {
        return a
    }
    if b > a {
        return b
    }
    if a == 0 {
        if !math.Signbit(a) || !math.Signbit(b) {
            return 0
        }
        return math.Copysign(0, -1)
    }
    return a
}
