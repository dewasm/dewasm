def i32_rotl(a, b)
  r = b & 31
  ((a << r) | (a >> (32 - r))) & M32
end
