func (rt) f32_min(a, b float32) float32 {
    if math.IsNaN(float64(a)) || math.IsNaN(float64(b)) {
        return float32(math.NaN())
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
