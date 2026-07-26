func (rt) f32_abs(x float32) float32 {
    return math.Float32frombits(math.Float32bits(x) &^ (1 << 31))
}
