rt_i32_trunc_sat_f64_s() {
  local a=$1 pa s e m t v
  (( pa = a & 0x7fffffffffffffff, s = (a >> 63) & 1 ))
  if (( pa > 0x7ff0000000000000 )); then
    R0=0
    return 0
  fi
  (( e = (pa >> 52) - 1023 ))
  if (( e < 0 )); then
    R0=0
    return 0
  fi
  if (( e >= 53 )); then
    R0=$(( s ? 0x80000000 : 0x7fffffff ))
    return 0
  fi
  (( m = (pa & 0xfffffffffffff) | 1 << 52, t = m >> (52 - e), v = s ? -t : t ))
  if (( v < -2147483648 )); then
    R0=$(( 0x80000000 ))
  elif (( v > 2147483647 )); then
    R0=$(( 0x7fffffff ))
  else
    R0=$(( v & 0xffffffff ))
  fi
  return 0
}
