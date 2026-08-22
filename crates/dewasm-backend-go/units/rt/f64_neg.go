func (rt) f64_neg(x float64) float64 {
    return math.Float64frombits(math.Float64bits(x) ^ (1 << 63))
}
