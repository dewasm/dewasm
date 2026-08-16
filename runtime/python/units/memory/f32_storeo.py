# requires: memory/i32_store, rt/f32_bits
def f32_storeo(self, a, off, v):
    self.i32_store(a + off, Rt.f32_bits(v))
