func (m *Memory) i32_store8(a uint64, v uint32) {
    if a+1 > m.n {
        panic(oobTrap)
    }
    *(*uint8)(unsafe.Add(m.base, a)) = uint8(v)
}
