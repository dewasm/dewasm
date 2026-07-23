def i32_clz(x)
  x == 0 ? 32 : 32 - x.bit_length
end
