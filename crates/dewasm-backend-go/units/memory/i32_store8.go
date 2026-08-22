func (m *Memory) i32_store8(a uint64, v uint32) {
    m.check(a, 1)
    m.data[a] = byte(v)
}
