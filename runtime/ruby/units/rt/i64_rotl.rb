# requires: rt/low_mask
def i64_rotl(a, b)
  r = b & 63
  ((a & LOW_MASK[64 - r]) << r) | (a >> (64 - r))
end
