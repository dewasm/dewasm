# requires: rt/m64
def i64_rotl(a, b)
  r = b & 63
  m64((a << r) | (a >> (64 - r)))
end
