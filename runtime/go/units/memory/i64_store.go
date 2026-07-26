func (m *Memory) i64_store(a uint64, v uint64) {
    m.check(a, 8)
    binary.LittleEndian.PutUint64(m.data[a:a+8], v)
}
