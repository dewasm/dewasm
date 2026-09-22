func (m *Memory) i32_load(a uint64) uint32 {
    if a+4 > m.n {
        panic(oobTrap)
    }
    return *(*uint32)(unsafe.Add(m.base, a))
}
