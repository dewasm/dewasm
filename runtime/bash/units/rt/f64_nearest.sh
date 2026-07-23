# Round to nearest integral, ties to even; +-0.5 round to +-0.
rt_f64_nearest() {
  local a=$1 pa s e mask frac half
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
    if (( pa <= 0x3fe0000000000000 )); then
      R0=$(( s << 63 ))
    else
      R0=$(( (s << 63) | 0x3ff0000000000000 ))
    fi
    return 0
  fi
  (( mask = (1 << (52 - e)) - 1, frac = pa & mask ))
  if (( frac == 0 )); then
    R0=$(( a ))
    return 0
  fi
  (( half = 1 << (51 - e) ))
  if (( frac > half || (frac == half && ((pa >> (52 - e)) & 1)) )); then
    R0=$(( (s << 63) | ((pa & ~mask) + mask + 1) ))
  else
    R0=$(( (s << 63) | (pa & ~mask) ))
  fi
  return 0
}
