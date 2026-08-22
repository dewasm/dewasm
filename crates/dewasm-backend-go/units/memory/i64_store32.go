func (m *Memory) i64_store32(a uint64, v uint64) {
    m.check(a, 4)
    binary.LittleEndian.PutUint32(m.data[a:a+4], uint32(v))
}
