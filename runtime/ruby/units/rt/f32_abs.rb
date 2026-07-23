# requires: rt/f32_bits, rt/f32_from_bits
def f32_abs(x) = f32_from_bits(f32_bits(x) & 0x7fff_ffff)
