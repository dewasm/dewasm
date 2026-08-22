// requires: memory/size
func (m *Memory) grow(delta uint32) uint32 {
    old := m.size()
    if uint64(old)+uint64(delta) > uint64(m.maxPages) {
        return 0xffffffff
    }
    m.data = append(m.data, make([]byte, uint64(delta)*65536)...)
    return old
}
