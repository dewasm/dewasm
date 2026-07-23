def i64_ctz(x)
  x == 0 ? 64 : (x & -x).bit_length - 1
end
