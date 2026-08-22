# requires: rt/low_mask
def i64_rotr(a, b)
  r = b & 63
  (a >> r) | ((a & LOW_MASK[r]) << (64 - r))
end
