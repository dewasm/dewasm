func (rt) i32_popcnt(x uint32) uint32 {
    return uint32(bits.OnesCount32(x))
}
