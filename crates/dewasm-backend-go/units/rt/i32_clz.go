func (rt) i32_clz(x uint32) uint32 {
    return uint32(bits.LeadingZeros32(x))
}
