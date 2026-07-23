# requires: rt/f64_promote, rt/i64_trunc_f64_u
rt_i64_trunc_f32_u() {
  rt_f64_promote "$1"
  rt_i64_trunc_f64_u "$R0" || return $?
  return 0
}
