# requires: rt/f32_bits, rt/f32_from_bits
def f32_neg(x) = f32_from_bits(f32_bits(x) ^ 0x8000_0000)
