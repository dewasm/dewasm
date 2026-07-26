// `//go:noinline` keeps Go from folding `x / 1.0` (or `x / -1.0`) to `x`
// (resp. `-x`), which would skip the signaling-NaN quieting wasm requires
// (ADR-2).
//go:noinline
func (rt) f32_div(a, b float32) float32 {
	return a / b
}
