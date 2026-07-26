func (m *Memory) i64_store8(a uint64, v uint64) {
    m.check(a, 1)
    m.data[a] = byte(v)
}
