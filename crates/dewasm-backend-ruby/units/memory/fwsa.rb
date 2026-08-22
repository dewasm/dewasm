# requires: memory/iwsa, rt/f32_bits, rt/trap
# Writing :f32 is bit-exact for every non-NaN value.
# A NaN takes the bit path because the double-to-float conversion quietens it, and wasm's f32.store is bit-preserving.
def fwsa(a, b, v) = v.nan? ? iwsa(a, b, Rt.f32_bits(v)) : (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.set_value(:f32, a, v))
