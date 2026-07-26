func (m *Memory) i64_store16(a uint64, v uint64) {
    m.check(a, 2)
    binary.LittleEndian.PutUint16(m.data[a:a+2], uint16(v))
}
