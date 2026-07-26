func (m *Memory) i32_load(a uint64) uint32 {
    m.check(a, 4)
    return binary.LittleEndian.Uint32(m.data[a : a+4])
}
