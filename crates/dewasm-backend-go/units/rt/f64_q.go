// requires: rt/f64_bits, rt/f64_from_bits
// The f64 counterpart of f32_q.
func (rt) f64_q(x float64) float64 {
    if x != x {
        return Rt.f64_from_bits(Rt.f64_bits(x) | 0x8000000000000)
    }
    return x
}
