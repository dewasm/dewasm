# requires: rt/f64_promote, rt/i32_trunc_sat_f64_s
rt_i32_trunc_sat_f32_s() {
  rt_f64_promote "$1"
  rt_i32_trunc_sat_f64_s "$R0"
  return 0
}
