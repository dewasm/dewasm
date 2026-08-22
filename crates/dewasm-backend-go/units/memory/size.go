func (m *Memory) size() uint32 {
    return uint32(len(m.data) / 65536)
}
