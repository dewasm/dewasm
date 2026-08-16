# requires: memory/iwlo, rt/f32_from_bits
# Goes through the bit-exact helper to preserve NaN sign/payload.
def fwlo(self, a, off):
    return Rt.f32_from_bits(self.iwlo(a, off))
