# Rounding toward zero on the pattern: clear the sub-integer fraction bits.
# Negative inputs rounding to zero yield -0 (Ruby ftrunc parity).
rt_f64_trunc() {
  local a=$1 pa s e mask
  (( pa = a & 0x7fffffffffffffff, s = (a >> 63) & 1 ))
  if (( pa > 0x7ff0000000000000 )); then
    R0=$(( 0x7ff8000000000000 ))
    return 0
  fi
  (( e = (pa >> 52) - 1023 ))
  if (( e >= 52 )); then
    R0=$(( a ))
    return 0
  fi
  if (( e < 0 )); then
    R0=$(( s << 63 ))
    return 0
  fi
  (( mask = (1 << (52 - e)) - 1 ))
  R0=$(( (s << 63) | (pa & ~mask) ))
  return 0
}
