# See rt/f64_nearest; 23-bit fraction, u32 patterns.
rt_f32_nearest() {
  local a=$1 pa s e mask frac half
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
    if (( pa <= 0x3f000000 )); then
      R0=$(( s << 31 ))
    else
      R0=$(( (s << 31) | 0x3f800000 ))
    fi
    return 0
  fi
  (( mask = (1 << (23 - e)) - 1, frac = pa & mask ))
  if (( frac == 0 )); then
    R0=$(( a ))
    return 0
  fi
  (( half = 1 << (22 - e) ))
  if (( frac > half || (frac == half && ((pa >> (23 - e)) & 1)) )); then
    R0=$(( (s << 31) | ((pa & ~mask & 0xffffffff) + mask + 1) ))
  else
    R0=$(( (s << 31) | (pa & ~mask & 0xffffffff) ))
  fi
  return 0
}
