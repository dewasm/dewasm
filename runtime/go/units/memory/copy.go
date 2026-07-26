func (m *Memory) copy(dst, src, length uint64) {
    m.check(dst, length)
    m.check(src, length)
    if length == 0 {
        return
    }
    copy(m.data[dst:dst+length], m.data[src:src+length])
}
