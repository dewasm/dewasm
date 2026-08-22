func (rt) i32_trunc_sat_u(x float64) uint32 {
    if math.IsNaN(x) {
        return 0
    }
    t := math.Trunc(x)
    if t < 0 {
        return 0
    }
    if t > 4294967295 {
        return 4294967295
    }
    return uint32(t)
}
