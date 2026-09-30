// requires: rt/f32_bits, rt/f32_from_bits
// Force a NaN result quiet.
// Go rewrites `x * 1.0` and `x / 1.0` to `x`, and `x * -1.0` to `-x`.
// That skips the signaling-NaN quieting wasm requires of every arithmetic result.
// Generated code instead wraps a multiply or a divide with a constant operand in this.
func (rt) f32_q(x float32) float32 {
    if x != x {
        return Rt.f32_from_bits(Rt.f32_bits(x) | 0x400000)
    }
    return x
}
