def i64_ctz(x)
  return 64 if x == 0
  # `x & -x` keeps the lowest set bit; for bit 63 it is the one negative result.
  low = x & -x
  low < 0 ? 63 : low.bit_length - 1
end
