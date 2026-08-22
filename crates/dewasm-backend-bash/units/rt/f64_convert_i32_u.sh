# requires: rt/f64_round_pack
rt_f64_convert_i32_u() {
  rt_f64_round_pack 0 53 "$1" 0
  return 0
}
