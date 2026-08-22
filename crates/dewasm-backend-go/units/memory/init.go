// requires: rt/trap
// Also used to initialize active data segments at instantiation time.
func (m *Memory) init(dst uint64, data []byte, src, length uint64) {
    if src+length > uint64(len(data)) {
        Rt.trap("out of bounds memory access")
    }
    m.check(dst, length)
    if length == 0 {
        return
    }
    copy(m.data[dst:dst+length], data[src:src+length])
}
