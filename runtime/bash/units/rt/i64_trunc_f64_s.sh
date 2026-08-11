# requires: rt/trap
# All range checks happen on the exponent BEFORE any shift: e-52 can reach 971 and bash takes shift counts mod 64.
rt_i64_trunc_f64_s() {
  local a=$1 pa s e m t
  (( pa = a & 0x7fffffffffffffff, s = (a >> 63) & 1 ))
  if (( pa > 0x7ff0000000000000 )); then
    rt_trap 'invalid conversion to integer'
    return $?
  fi
  if (( pa == 0x7ff0000000000000 )); then
    rt_trap 'integer overflow'
    return $?
  fi
  (( e = (pa >> 52) - 1023 ))
  if (( e < 0 )); then
    R0=0
    return 0
  fi
  if (( e >= 63 )); then
    if (( e == 63 && s == 1 && (pa & 0xfffffffffffff) == 0 )); then
      R0=$(( 1 << 63 ))
      return 0
    fi
    rt_trap 'integer overflow'
    return $?
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
