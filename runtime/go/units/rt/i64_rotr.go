func (rt) i64_rotr(a, b uint64) uint64 {
    r := b & 63
    return a>>r | a<<((64-r)&63)
}
