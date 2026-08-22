# requires: rt/f32_from_bits, rt/trap
# Reading :f32 is bit-exact for every non-NaN value.
# A NaN takes the bit path because the float-to-double conversion quietens it, and wasm's f32.load is bit-preserving.
def fwlo(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 4 > @size; r = @buffer.get_value(:f32, a); r.nan? ? Rt.f32_from_bits(@buffer.get_value(:u32, a)) : r)
