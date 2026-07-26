func (rt) i32_ctz(x uint32) uint32 {
    return uint32(bits.TrailingZeros32(x))
}
