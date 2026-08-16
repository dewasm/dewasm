# requires: memory/i32_loada, rt/f32_from_bits
# Goes through the bit-exact helper to preserve NaN sign/payload.
def f32_loada(self, a, b):
    return Rt.f32_from_bits(self.i32_loada(a, b))
