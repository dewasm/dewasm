// requires: rt/trap
func (rt) i32_rem_s(a, b uint32) uint32 {
    if b == 0 {
        Rt.trap("integer divide by zero")
    }
    if b == 0xffffffff {
        return 0
    }
    return uint32(int32(a) % int32(b))
}
