// requires: rt/trap
func (rt) i32_trunc_s(x float64) uint32 {
    if math.IsNaN(x) {
        Rt.trap("invalid conversion to integer")
    }
    if math.IsInf(x, 0) {
        Rt.trap("integer overflow")
    }
    t := math.Trunc(x)
    if t < -2147483648 || t > 2147483647 {
        Rt.trap("integer overflow")
    }
    return uint32(int32(t))
}
