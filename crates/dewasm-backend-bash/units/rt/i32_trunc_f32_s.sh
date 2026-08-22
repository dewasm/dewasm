# requires: rt/f64_promote, rt/i32_trunc_f64_s
# Promote is exact, so this is still a single truncation.
rt_i32_trunc_f32_s() {
  rt_f64_promote "$1"
  rt_i32_trunc_f64_s "$R0" || return $?
  return 0
}
