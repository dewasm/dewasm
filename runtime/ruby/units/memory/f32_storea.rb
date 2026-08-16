# requires: memory/i32_storea, rt/f32_bits
def f32_storea(a, b, v) = i32_storea(a, b, Rt.f32_bits(v))
