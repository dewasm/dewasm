# requires: memory/i32_storea, rt/f32_bits
def f32_storea(self, a, b, v):
    self.i32_storea(a, b, Rt.f32_bits(v))
