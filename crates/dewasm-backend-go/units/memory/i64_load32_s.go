// requires: memory/i64_load32_u
func (m *Memory) i64_load32_s(a uint64) uint64 {
    return uint64(int64(int32(m.i64_load32_u(a))))
}
