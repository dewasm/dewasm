func (m *Memory) fill(dst, val, length uint64) {
    m.check(dst, length)
    for i := uint64(0); i < length; i++ {
        m.data[dst+i] = byte(val)
    }
}
