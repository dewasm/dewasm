# requires: memory/iwso, rt/f32_bits
def fwso(self, a, off, v):
    self.iwso(a, off, Rt.f32_bits(v))
