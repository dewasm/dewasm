// requires: rt/trap
func (rt) i32_div_s(a, b uint32) uint32 {
    if b == 0 {
        Rt.trap("integer divide by zero")
    }
    if a == 0x80000000 && b == 0xffffffff {
        Rt.trap("integer overflow")
    }
    return uint32(int32(a) / int32(b))
}
