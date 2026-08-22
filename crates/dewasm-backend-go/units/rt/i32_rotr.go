func (rt) i32_rotr(a, b uint32) uint32 {
    r := b & 31
    return a>>r | a<<((32-r)&31)
}
