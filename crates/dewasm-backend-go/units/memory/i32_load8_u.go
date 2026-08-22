func (m *Memory) i32_load8_u(a uint64) uint32 {
    m.check(a, 1)
    return uint32(m.data[a])
}
