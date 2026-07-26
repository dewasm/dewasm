// requires: memory/i32_store, rt/f32_bits
func (m *Memory) f32_store(a uint64, v float32) {
    m.i32_store(a, Rt.f32_bits(v))
}
