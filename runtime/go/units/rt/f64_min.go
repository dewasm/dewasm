func (rt) f64_min(a, b float64) float64 {
    if math.IsNaN(a) || math.IsNaN(b) {
        return math.NaN()
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
