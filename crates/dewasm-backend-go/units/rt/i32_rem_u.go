// requires: rt/trap
func (rt) i32_rem_u(a, b uint32) uint32 {
    if b == 0 {
        Rt.trap("integer divide by zero")
    }
    return a % b
}
