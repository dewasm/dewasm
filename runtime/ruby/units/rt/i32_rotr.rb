def i32_rotr(a, b)
  r = b & 31
  ((a >> r) | (a << (32 - r))) & M32
end
