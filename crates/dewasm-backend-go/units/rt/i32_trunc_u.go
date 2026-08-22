// requires: rt/trap
func (rt) i32_trunc_u(x float64) uint32 {
    if math.IsNaN(x) {
        Rt.trap("invalid conversion to integer")
    }
    if math.IsInf(x, 0) {
        Rt.trap("integer overflow")
    }
    t := math.Trunc(x)
    if t < 0 || t > 4294967295 {
        Rt.trap("integer overflow")
    }
    return uint32(t)
}
