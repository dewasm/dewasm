# requires: rt/f32, rt/cvt_f32_i
# Converts a word read as unsigned to f32 with one rounding.
# A negative word is 2**63 or more: its top 53 bits are kept, with any lost 1 in the last bit.
def cvt_f32_u(v)
  return cvt_f32_i(v) if v >= 0
  hi = (v >> 11) & 0x001f_ffff_ffff_ffff
  hi |= 1 if (v & 0x7ff) != 0
  f32(hi.to_f * 2048.0)
end
