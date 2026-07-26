# requires: memory/i32_load, rt/f32_from_bits
# Goes through the bit-exact helper to preserve NaN sign/payload (ADR-2).
def f32_load(self, a):
    return Rt.f32_from_bits(self.i32_load(a))
