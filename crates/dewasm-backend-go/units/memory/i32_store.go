func (m *Memory) i32_store(a uint64, v uint32) {
    if a+4 > m.n {
        panic(oobTrap)
    }
    *(*uint32)(unsafe.Add(m.base, a)) = v
}
