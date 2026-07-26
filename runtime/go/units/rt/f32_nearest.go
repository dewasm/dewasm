func (rt) f32_nearest(x float32) float32 {
    return float32(math.RoundToEven(float64(x)))
}
