# requires: memory/i32_load, rt/f32_from_bits
# Goes through the bit-exact helper to preserve NaN sign/payload.
def f32_loado(self, a, off):
    return Rt.f32_from_bits(self.i32_load(a + off))
