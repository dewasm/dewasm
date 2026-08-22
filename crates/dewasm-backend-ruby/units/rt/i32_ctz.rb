def i32_ctz(x)
  x == 0 ? 32 : (x & -x).bit_length - 1
end
