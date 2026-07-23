# requires: rt/f64_promote, rt/i64_trunc_sat_f64_u
rt_i64_trunc_sat_f32_u() {
  rt_f64_promote "$1"
  rt_i64_trunc_sat_f64_u "$R0"
  return 0
}
