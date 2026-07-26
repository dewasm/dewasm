func (m *Memory) i64_load8_u(a uint64) uint64 {
    m.check(a, 1)
    return uint64(m.data[a])
}
