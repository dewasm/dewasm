// requires: memory/i64_load8_u
func (m *Memory) i64_load8_s(a uint64) uint64 {
    return uint64(int64(int8(m.i64_load8_u(a))))
}
