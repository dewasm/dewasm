// requires: memory/i32_load, rt/f32_from_bits
func (m *Memory) f32_load(a uint64) float32 {
    return Rt.f32_from_bits(m.i32_load(a))
}
