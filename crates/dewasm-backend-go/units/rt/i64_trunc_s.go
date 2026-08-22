// requires: rt/trap
func (rt) i64_trunc_s(x float64) uint64 {
    if math.IsNaN(x) {
        Rt.trap("invalid conversion to integer")
    }
    if math.IsInf(x, 0) {
        Rt.trap("integer overflow")
    }
    t := math.Trunc(x)
    if t < -9223372036854775808.0 || t >= 9223372036854775808.0 {
        Rt.trap("integer overflow")
    }
    return uint64(int64(t))
}
