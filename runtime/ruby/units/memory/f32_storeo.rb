# requires: memory/i32_storeo, rt/f32_bits
def f32_storeo(a, off, v) = i32_storeo(a, off, Rt.f32_bits(v))
