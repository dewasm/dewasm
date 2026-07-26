func (rt) i64_ctz(x uint64) uint64 {
    return uint64(bits.TrailingZeros64(x))
}
