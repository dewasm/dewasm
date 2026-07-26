func (rt) f64_max(a, b float64) float64 {
    if math.IsNaN(a) || math.IsNaN(b) {
        return math.NaN()
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
