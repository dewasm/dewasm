# requires: rt/f32
# Converts a signed word to f32 with one rounding.
# A magnitude of 2**53 or more first loses its low bits, with any lost 1 kept in the last bit.
# The double is then exact, so the step from double to single is the only rounding.
def cvt_f32_i(v)
  return f32(v.to_f) if v > -9007199254740992 && v < 9007199254740992
  return -9223372036854775808.0 if v == I64_MIN
  a = v < 0 ? -v : v
  sh = a.bit_length - 53
  hi = a >> sh
  hi |= 1 if a != (hi << sh)
  r = f32(hi.to_f * (1 << sh).to_f)
  v < 0 ? -r : r
end
