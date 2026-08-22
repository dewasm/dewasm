# See rt/i64_trunc_f64_u for the deliberate e == 63 wrap.
rt_i64_trunc_sat_f64_u() {
  local a=$1 pa s e m
  (( pa = a & 0x7fffffffffffffff, s = (a >> 63) & 1 ))
  if (( pa > 0x7ff0000000000000 )); then
    R0=0
    return 0
  fi
  (( e = (pa >> 52) - 1023 ))
  if (( e < 0 || s == 1 )); then
    R0=0
    return 0
  fi
  if (( e >= 64 )); then
    R0=-1
    return 0
  fi
  (( m = (pa & 0xfffffffffffff) | 1 << 52 ))
  if (( e <= 52 )); then
    R0=$(( m >> (52 - e) ))
  else
    R0=$(( m << (e - 52) ))
  fi
  return 0
}
