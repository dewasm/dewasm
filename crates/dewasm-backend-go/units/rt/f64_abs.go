func (rt) f64_abs(x float64) float64 {
    return math.Float64frombits(math.Float64bits(x) &^ (1 << 63))
}
