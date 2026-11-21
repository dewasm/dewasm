# requires: rt/i64_shr_u
def i64_rotr(a, b)
  r = b & 63
  r == 0 ? a : i64_shr_u(a, r) | (a << (64 - r))
end
