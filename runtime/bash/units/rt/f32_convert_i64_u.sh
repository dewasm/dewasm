# requires: rt/f32_round_pack
# See rt/f64_convert_i64_u for the exact-halving argument.
rt_f32_convert_i64_u() {
  local v=$1
  if (( v < 0 )); then
    rt_f32_round_pack 0 25 $(( (v >> 1) & 0x7fffffffffffffff )) $(( v & 1 ))
  else
    rt_f32_round_pack 0 24 "$v" 0
  fi
  return 0
}
