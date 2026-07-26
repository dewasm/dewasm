// wasm has no float contraction and requires identity ops to quiet signaling
// NaNs. `//go:noinline` keeps a constant operand from reaching the multiply, so
// Go's compiler can neither fold `x * 1.0` to `x` (which would skip the sNaN
// quieting) nor fuse a following add/sub into a hardware FMA (ADR-2).
//go:noinline
func (rt) f32_mul(a, b float32) float32 {
	return a * b
}
