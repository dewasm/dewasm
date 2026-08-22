// requires: rt/trap
func (rt) i64_div_u(a, b uint64) uint64 {
    if b == 0 {
        Rt.trap("integer divide by zero")
    }
    return a / b
}
