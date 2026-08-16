# requires: memory/iwla, rt/f32_from_bits
# Goes through the bit-exact helper to preserve NaN sign/payload.
def fwla(self, a, b):
    return Rt.f32_from_bits(self.iwla(a, b))
