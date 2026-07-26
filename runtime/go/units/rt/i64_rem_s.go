// requires: rt/trap
func (rt) i64_rem_s(a, b uint64) uint64 {
    if b == 0 {
        Rt.trap("integer divide by zero")
    }
    if b == 0xffffffffffffffff {
        return 0
    }
    return uint64(int64(a) % int64(b))
}
