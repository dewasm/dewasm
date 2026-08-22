# requires: rt/f64_bits, rt/f64_from_bits
def f64_copysign(a, b)
  f64_from_bits((f64_bits(a) & 0x7fff_ffff_ffff_ffff) | (f64_bits(b) & 0x8000_0000_0000_0000))
end
