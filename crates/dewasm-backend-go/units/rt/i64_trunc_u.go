// requires: rt/trap
func (rt) i64_trunc_u(x float64) uint64 {
    if math.IsNaN(x) {
        Rt.trap("invalid conversion to integer")
    }
    if math.IsInf(x, 0) {
        Rt.trap("integer overflow")
    }
    t := math.Trunc(x)
    if t < 0 || t >= 18446744073709551616.0 {
        Rt.trap("integer overflow")
    }
    return uint64(t)
}
