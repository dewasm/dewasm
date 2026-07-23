# requires: rt/f64_promote, rt/f64_sqrt, rt/f32_demote
rt_f32_sqrt() {
  rt_f64_promote "$1"
  rt_f64_sqrt "$R0"
  rt_f32_demote "$R0"
  return 0
}
