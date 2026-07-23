# requires: rt/f32, rt/f32_bits, rt/f32_from_bits
def f32_demote(x)
  x.nan? ? f32_from_bits((f32_bits(x) & 0x8000_0000) | 0x7fc0_0000) : f32(x)
end
