# requires: rt/f64_promote, rt/f64_div, rt/f32_demote
rt_f32_div() {
  local x
  rt_f64_promote "$1"
  x=$R0
  rt_f64_promote "$2"
  rt_f64_div "$x" "$R0"
  rt_f32_demote "$R0"
  return 0
}
