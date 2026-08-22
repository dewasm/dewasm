// requires: memory/i32_load16_u
func (m *Memory) i32_load16_s(a uint64) uint32 {
    return uint32(int32(int16(m.i32_load16_u(a))))
}
