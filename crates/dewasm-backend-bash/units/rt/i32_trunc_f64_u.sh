# requires: rt/trap
rt_i32_trunc_f64_u() {
  local a=$1 pa s e m t v
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
  if (( e >= 53 )); then
    rt_trap 'integer overflow'
    return $?
  fi
  (( m = (pa & 0xfffffffffffff) | 1 << 52, t = m >> (52 - e), v = s ? -t : t ))
  if (( v < 0 || v > 0xffffffff )); then
    rt_trap 'integer overflow'
    return $?
  fi
  R0=$(( v ))
  return 0
}
