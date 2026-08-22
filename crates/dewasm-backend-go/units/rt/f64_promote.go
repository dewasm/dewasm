func (rt) f64_promote(x float32) float64 {
    if math.IsNaN(float64(x)) {
        b := math.Float32bits(x)
        return math.Float64frombits(uint64(b>>31)<<63 | 0x7ff8000000000000 | uint64(b&0x7fffff)<<29)
    }
    return float64(x)
}
