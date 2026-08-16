# requires: memory/i32_loada, rt/f32_from_bits
# The bit path preserves a NaN's sign and payload.
def f32_loada(a, b) = Rt.f32_from_bits(i32_loada(a, b))
