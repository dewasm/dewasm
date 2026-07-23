def i64_rotl(a, b)
  r = b & 63
  ((a << r) | (a >> (64 - r))) & M64
end
