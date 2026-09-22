func (m *Memory) i32_load8_u(a uint64) uint32 {
    if a+1 > m.n {
        panic(oobTrap)
    }
    return uint32(*(*uint8)(unsafe.Add(m.base, a)))
}
