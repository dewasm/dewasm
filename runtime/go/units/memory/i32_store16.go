func (m *Memory) i32_store16(a uint64, v uint32) {
    m.check(a, 2)
    binary.LittleEndian.PutUint16(m.data[a:a+2], uint16(v))
}
