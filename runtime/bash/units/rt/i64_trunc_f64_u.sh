# requires: rt/trap
# At e == 63 the m << 11 deliberately wraps negative: that wrapped value
# is exactly the u64 bit pattern of the result (ADR-11 representation).
rt_i64_trunc_f64_u() {
  local a=$1 pa s e m
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
  if (( s == 1 )); then
    rt_trap 'integer overflow'
    return $?
  fi
  if (( e >= 64 )); then
    rt_trap 'integer overflow'
    return $?
  fi
  (( m = (pa & 0xfffffffffffff) | 1 << 52 ))
  if (( e <= 52 )); then
    R0=$(( m >> (52 - e) ))
  else
    R0=$(( m << (e - 52) ))
  fi
  return 0
}
