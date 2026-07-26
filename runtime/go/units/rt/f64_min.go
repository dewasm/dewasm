func (rt) f64_min(a, b float64) float64 {
    // A NaN operand yields the wasm canonical NaN (always a legal min/max
    // result and, being quiet, satisfies nan:arithmetic too); Go's math.NaN()
    // is not bit-canonical (its low mantissa bit is set), so build it
    // explicitly (ADR-2).
    if math.IsNaN(a) || math.IsNaN(b) {
        return math.Float64frombits(0x7ff8000000000000)
    }
    if a < b {
        return a
    }
    if b < a {
        return b
    }
    if a == 0 {
        if math.Signbit(a) || math.Signbit(b) {
            return math.Copysign(0, -1)
        }
        return 0
    }
    return a
}
