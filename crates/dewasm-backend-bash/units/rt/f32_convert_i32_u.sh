# requires: rt/f32_round_pack
rt_f32_convert_i32_u() {
  rt_f32_round_pack 0 24 "$1" 0
  return 0
}
