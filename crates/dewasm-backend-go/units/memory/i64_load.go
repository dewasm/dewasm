func (m *Memory) i64_load(a uint64) uint64 {
    if a+8 > m.n {
        panic(oobTrap)
    }
    return *(*uint64)(unsafe.Add(m.base, a))
}
