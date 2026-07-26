func (rt) i64_clz(x uint64) uint64 {
    return uint64(bits.LeadingZeros64(x))
}
