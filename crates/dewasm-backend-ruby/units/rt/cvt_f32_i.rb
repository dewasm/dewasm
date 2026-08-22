# requires: rt/f32
# Convert a (signed) Integer to f32 with correct rounding.
# Values beyond
# 2**53 are pre-rounded to odd so that the double->single step cannot double-round.
def cvt_f32_i(v)
  a = v.abs
  if a < (1 << 53)
    f32(v.to_f)
  else
    sh = a.bit_length - 53
    hi = a >> sh
    hi |= 1 if a != (hi << sh)
    hi = -hi if v < 0
    f32(hi.to_f * (2.0**sh))
  end
end
