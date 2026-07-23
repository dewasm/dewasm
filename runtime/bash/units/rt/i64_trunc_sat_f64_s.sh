# The exact -2^63 case needs no special path: the negative clamp is the
# same 1<<63 pattern.
rt_i64_trunc_sat_f64_s() {
  local a=$1 pa s e m t
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
  if (( e >= 63 )); then
    R0=$(( s ? 1 << 63 : 0x7fffffffffffffff ))
    return 0
  fi
  (( m = (pa & 0xfffffffffffff) | 1 << 52 ))
  if (( e <= 52 )); then
    (( t = m >> (52 - e) ))
  else
    (( t = m << (e - 52) ))
  fi
  R0=$(( s ? -t : t ))
  return 0
}
