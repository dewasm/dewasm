func (rt) f32_copysign(a, b float32) float32 {
    return math.Float32frombits(math.Float32bits(a)&^(1<<31) | math.Float32bits(b)&(1<<31))
}
