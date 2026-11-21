# requires: rt/i64_shr_u
def i64_rotl(a, b)
  r = b & 63
  r == 0 ? a : (a << r) | i64_shr_u(a, 64 - r)
end
