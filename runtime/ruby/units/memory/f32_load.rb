# requires: memory/i32_load, rt/f32_from_bits
# Goes through the bit-exact conversion helpers to preserve NaN
# sign/payload (see rt/f32_bits).
def f32_load(a) = Rt.f32_from_bits(i32_load(a))
