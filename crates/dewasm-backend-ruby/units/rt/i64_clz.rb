def i64_clz(x)
  x == 0 ? 64 : 64 - x.bit_length
end
