// requires: memory/i64_load16_u
func (m *Memory) i64_load16_s(a uint64) uint64 {
    return uint64(int64(int16(m.i64_load16_u(a))))
}
