# requires: rt/f32_round_pack
# Direct single rounding from the 64-bit integer (no f64 hop, so no
# round-to-odd pre-step is needed); INT64_MIN as in rt/f64_convert_i64_s.
rt_f32_convert_i64_s() {
  local v=$1 s=0
  if (( v == 1 << 63 )); then
    R0=$(( 0xDF000000 ))
    return 0
  fi
  if (( v < 0 )); then
    (( s = 1, v = -v ))
  fi
  rt_f32_round_pack "$s" 24 "$v" 0
  return 0
}
