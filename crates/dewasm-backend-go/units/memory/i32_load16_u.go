func (m *Memory) i32_load16_u(a uint64) uint32 {
    if a+2 > m.n {
        panic(oobTrap)
    }
    return uint32(*(*uint16)(unsafe.Add(m.base, a)))
}
