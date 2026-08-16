# requires: memory/iwl, rt/f32_from_bits
# Goes through the bit-exact helper to preserve NaN sign/payload.
def fwl(self, a):
    return Rt.f32_from_bits(self.iwl(a))
