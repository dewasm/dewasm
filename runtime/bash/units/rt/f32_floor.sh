# See rt/f64_floor; 23-bit fraction, u32 patterns.
rt_f32_floor() {
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
    if (( s == 1 && pa != 0 )); then
      R0=$(( 0xbf800000 ))
    else
      R0=$(( s << 31 ))
    fi
    return 0
  fi
  (( mask = (1 << (23 - e)) - 1 ))
  if (( (pa & mask) == 0 )); then
    R0=$(( a ))
    return 0
  fi
  if (( s == 1 )); then
    R0=$(( (s << 31) | ((pa & ~mask & 0xffffffff) + mask + 1) ))
  else
    R0=$(( pa & ~mask & 0xffffffff ))
  fi
  return 0
}
