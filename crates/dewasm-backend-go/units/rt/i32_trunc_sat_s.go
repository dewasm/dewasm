func (rt) i32_trunc_sat_s(x float64) uint32 {
    if math.IsNaN(x) {
        return 0
    }
    t := math.Trunc(x)
    if t < -2147483648 {
        return 2147483648
    }
    if t > 2147483647 {
        return 2147483647
    }
    return uint32(int32(t))
}
