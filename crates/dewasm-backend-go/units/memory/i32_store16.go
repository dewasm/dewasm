func (m *Memory) i32_store16(a uint64, v uint32) {
    if a+2 > m.n {
        panic(oobTrap)
    }
    *(*uint16)(unsafe.Add(m.base, a)) = uint16(v)
}
