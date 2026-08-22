# requires: memory/iwso, rt/f32_bits, rt/trap
# Writing :f32 is bit-exact for every non-NaN value.
# A NaN takes the bit path because the double-to-float conversion quietens it, and wasm's f32.store is bit-preserving.
def fwso(a, off, v) = v.nan? ? iwso(a, off, Rt.f32_bits(v)) : (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.set_value(:f32, a, v))
