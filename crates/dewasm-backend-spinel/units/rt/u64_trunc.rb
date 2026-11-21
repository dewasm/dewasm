# `x` is above -1.0 and below 2**64.
# From 2**63 up, the difference from 2**63 is exact, and it fits the low 63 bits.
def u64_trunc(x)
  x < 9223372036854775808.0 ? x.truncate : (x - 9223372036854775808.0).truncate | I64_MIN
end
