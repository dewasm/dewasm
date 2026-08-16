# requires: memory/iwsa, rt/f32_bits
def fwsa(self, a, b, v):
    self.iwsa(a, b, Rt.f32_bits(v))
