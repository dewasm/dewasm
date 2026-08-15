# requires: memory/i32_store, rt/f32_bits
def f32_storeo(a, off, v) = i32_store(a + off, Rt.f32_bits(v))
