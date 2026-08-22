func (rt) f64_copysign(a, b float64) float64 {
    return math.Float64frombits(math.Float64bits(a)&^(1<<63) | math.Float64bits(b)&(1<<63))
}
