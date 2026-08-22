def sext(x, bits, mask)
  half = 1 << (bits - 1)
  (((x & ((1 << bits) - 1)) ^ half) - half) & mask
end
