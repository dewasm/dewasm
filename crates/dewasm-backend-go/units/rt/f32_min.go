func (rt) f32_min(a, b float32) float32 {
    // A NaN operand yields the wasm canonical NaN (always a legal min/max result and, being quiet, satisfies nan:arithmetic too); Go's math.NaN()
    // is not bit-canonical, so build it explicitly.
    if math.IsNaN(float64(a)) || math.IsNaN(float64(b)) {
        return math.Float32frombits(0x7fc00000)
    }
    if a < b {
        return a
    }
    if b < a {
        return b
    }
    if a == 0 {
        if math.Signbit(float64(a)) || math.Signbit(float64(b)) {
            return float32(math.Copysign(0, -1))
        }
        return 0
    }
    return a
}
