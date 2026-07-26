func (m *Memory) i64_load(a uint64) uint64 {
    m.check(a, 8)
    return binary.LittleEndian.Uint64(m.data[a : a+8])
}
