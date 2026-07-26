# requires: memory/i32_store, rt/f32_bits
def f32_store(self, a, v):
    self.i32_store(a, Rt.f32_bits(v))
