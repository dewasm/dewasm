func (rt) i64_trunc_sat_u(x float64) uint64 {
    if math.IsNaN(x) {
        return 0
    }
    t := math.Trunc(x)
    if t < 0 {
        return 0
    }
    if t >= 18446744073709551616.0 {
        return 0xffffffffffffffff
    }
    return uint64(t)
}
