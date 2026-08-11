# The away-branch (pa & ~mask) + mask + 1 increments the magnitude by one integral step; a mantissa carry rolls into the exponent field, which is exactly right on patterns (e.g. -1.5 -> -2.0).
rt_f64_floor() {
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
    if (( s == 1 && pa != 0 )); then
      R0=$(( 0xbff0000000000000 ))
    else
      R0=$(( s << 63 ))
    fi
    return 0
  fi
  (( mask = (1 << (52 - e)) - 1 ))
  if (( (pa & mask) == 0 )); then
    R0=$(( a ))
    return 0
  fi
  if (( s == 1 )); then
    R0=$(( (s << 63) | ((pa & ~mask) + mask + 1) ))
  else
    R0=$(( pa & ~mask ))
  fi
  return 0
}
