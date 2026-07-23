rt_i32_trunc_sat_f64_u() {
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
    R0=$(( s ? 0 : 0xffffffff ))
    return 0
  fi
  (( m = (pa & 0xfffffffffffff) | 1 << 52, t = m >> (52 - e), v = s ? -t : t ))
  if (( v < 0 )); then
    R0=0
  elif (( v > 0xffffffff )); then
    R0=$(( 0xffffffff ))
  else
    R0=$(( v ))
  fi
  return 0
}
