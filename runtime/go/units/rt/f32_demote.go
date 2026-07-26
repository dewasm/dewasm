func (rt) f32_demote(x float64) float32 {
    if math.IsNaN(x) {
        b := math.Float64bits(x)
        return math.Float32frombits(uint32(b>>32)&0x80000000 | 0x7fc00000)
    }
    return float32(x)
}
