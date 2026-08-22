func (rt) i64_trunc_sat_s(x float64) uint64 {
    if math.IsNaN(x) {
        return 0
    }
    t := math.Trunc(x)
    if t < -9223372036854775808.0 {
        return 0x8000000000000000
    }
    if t >= 9223372036854775808.0 {
        return 0x7fffffffffffffff
    }
    return uint64(int64(t))
}
