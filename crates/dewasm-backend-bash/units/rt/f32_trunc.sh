# See rt/f64_trunc; 23-bit fraction, u32 patterns.
rt_f32_trunc() {
  local a=$1 pa s e mask
  (( pa = a & 0x7fffffff, s = (a >> 31) & 1 ))
  if (( pa > 0x7f800000 )); then
    R0=$(( 0x7fc00000 ))
    return 0
  fi
  (( e = (pa >> 23) - 127 ))
  if (( e >= 23 )); then
    R0=$(( a ))
    return 0
  fi
  if (( e < 0 )); then
    R0=$(( s << 31 ))
    return 0
  fi
  (( mask = (1 << (23 - e)) - 1 ))
  R0=$(( (s << 31) | (pa & ~mask & 0xffffffff) ))
  return 0
}
