# requires: memory/i32_load, rt/f32_from_bits
# The bit path preserves a NaN's sign and payload.
def f32_loado(a, off) = Rt.f32_from_bits(i32_load(a + off))
