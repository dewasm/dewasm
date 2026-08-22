func (rt) i64_popcnt(x uint64) uint64 {
    return uint64(bits.OnesCount64(x))
}
