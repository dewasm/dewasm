func (m *Memory) i64_load16_u(a uint64) uint64 {
    m.check(a, 2)
    return uint64(binary.LittleEndian.Uint16(m.data[a : a+2]))
}
