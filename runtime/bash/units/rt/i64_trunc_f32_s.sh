# requires: rt/f64_promote, rt/i64_trunc_f64_s
rt_i64_trunc_f32_s() {
  rt_f64_promote "$1"
  rt_i64_trunc_f64_s "$R0" || return $?
  return 0
}
