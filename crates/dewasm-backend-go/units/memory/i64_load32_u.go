func (m *Memory) i64_load32_u(a uint64) uint64 {
    m.check(a, 4)
    return uint64(binary.LittleEndian.Uint32(m.data[a : a+4]))
}
