# Counts in parallel within 2-bit, 4-bit, and 8-bit fields, then sums the bytes by a multiply.
# Each mask also clears the sign bits a `>>` copies in, so a negative word counts right.
def popcnt(x)
  x -= (x >> 1) & 0x5555_5555_5555_5555
  x = (x & 0x3333_3333_3333_3333) + ((x >> 2) & 0x3333_3333_3333_3333)
  x = (x + (x >> 4)) & 0x0f0f_0f0f_0f0f_0f0f
  ((x * 0x0101_0101_0101_0101) >> 56) & 0xff
end
