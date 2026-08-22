// See f32_div: `//go:noinline` preserves the sNaN quieting of `x / 1.0` and
// `x / -1.0` that Go would otherwise fold away.
//go:noinline
func (rt) f64_div(a, b float64) float64 {
	return a / b
}
