# requires: rt/f32_round_pack
rt_f32_convert_i32_s() {
  local v=$(( ($1 ^ 0x80000000) - 0x80000000 )) s=0
  if (( v < 0 )); then
    (( s = 1, v = -v ))
  fi
  rt_f32_round_pack "$s" 24 "$v" 0
  return 0
}
