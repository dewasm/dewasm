func (m *Memory) i32_load16_u(a uint64) uint32 {
    m.check(a, 2)
    return uint32(binary.LittleEndian.Uint16(m.data[a : a+2]))
}
