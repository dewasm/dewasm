// requires: rt/trap
func (rt) i64_div_s(a, b uint64) uint64 {
    if b == 0 {
        Rt.trap("integer divide by zero")
    }
    if a == 0x8000000000000000 && b == 0xffffffffffffffff {
        Rt.trap("integer overflow")
    }
    return uint64(int64(a) / int64(b))
}
