func (m *Memory) i32_store(a uint64, v uint32) {
    m.check(a, 4)
    binary.LittleEndian.PutUint32(m.data[a:a+4], v)
}
