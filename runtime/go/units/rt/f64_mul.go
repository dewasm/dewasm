// See f32_mul: `//go:noinline` blocks both `x * 1.0 -> x` folding and FMA fusion, so each op rounds independently and quiets signaling NaNs.
//go:noinline
func (rt) f64_mul(a, b float64) float64 {
	return a * b
}
