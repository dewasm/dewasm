// requires: memory/i32_load8_u
func (m *Memory) i32_load8_s(a uint64) uint32 {
    return uint32(int32(int8(m.i32_load8_u(a))))
}
