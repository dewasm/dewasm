# requires: rt/f32_round_pack
# 53 -> 24 bit RNE. Ruby's 2^128 - 2^103 overflow boundary needs no code
# here: it is exactly where round-to-nearest starts mapping to infinity.
# NaN keeps its sign and goes canonical (Ruby f32_demote parity).
rt_f32_demote() {
  local a=$1 pa s ma ea
  (( pa = a & 0x7fffffffffffffff, s = (a >> 63) & 1 ))
  if (( pa > 0x7ff0000000000000 )); then
    R0=$(( (s << 31) | 0x7fc00000 ))
    return 0
  fi
  if (( pa == 0x7ff0000000000000 )); then
    R0=$(( (s << 31) | 0x7f800000 ))
    return 0
  fi
  if (( pa == 0 )); then
    R0=$(( s << 31 ))
    return 0
  fi
  if (( pa >> 52 )); then
    (( ma = (pa & 0xfffffffffffff) | 1 << 52, ea = (pa >> 52) - 1023 ))
  else
    (( ma = pa, ea = -1022 ))
  fi
  rt_f32_round_pack "$s" $(( ea - 28 )) "$ma" 0
  return 0
}
