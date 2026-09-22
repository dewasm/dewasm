func (m *Memory) i64_store(a uint64, v uint64) {
    if a+8 > m.n {
        panic(oobTrap)
    }
    *(*uint64)(unsafe.Add(m.base, a)) = v
}
