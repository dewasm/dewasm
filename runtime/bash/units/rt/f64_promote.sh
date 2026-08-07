# requires: rt/f64_round_pack
# Exact: every f32 value is representable in f64. NaN goes canonical
# (arithmetic-op policy); subnormal f32 left-normalizes through
# round_pack with zero sticky.
rt_f64_promote() {
  local b=$1 pb s
  (( pb = b & 0x7fffffff, s = (b >> 31) & 1 ))
  if (( pb > 0x7f800000 )); then
    R0=$(( 0x7ff8000000000000 ))
    return 0
  fi
  if (( pb == 0x7f800000 )); then
    R0=$(( (s << 63) | 0x7ff0000000000000 ))
    return 0
  fi
  if (( pb == 0 )); then
    R0=$(( s << 63 ))
    return 0
  fi
  if (( pb >> 23 )); then
    R0=$(( (s << 63) | ((((pb >> 23) - 127 + 1023)) << 52) | ((pb & 0x7fffff) << 29) ))
    return 0
  fi
  rt_f64_round_pack "$s" -96 "$pb" 0
  return 0
}
