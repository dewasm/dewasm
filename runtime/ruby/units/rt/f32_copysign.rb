# requires: rt/f32_bits, rt/f32_from_bits
def f32_copysign(a, b)
  f32_from_bits((f32_bits(a) & 0x7fff_ffff) | (f32_bits(b) & 0x8000_0000))
end
