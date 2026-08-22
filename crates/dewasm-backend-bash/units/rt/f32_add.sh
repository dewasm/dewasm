# requires: rt/f64_promote, rt/f64_add, rt/f32_demote
# f32 = demote(f64 op(promote, promote)): exact promote + 53 >= 2*24+2 makes the double rounding innocuous.
rt_f32_add() {
  local x
  rt_f64_promote "$1"
  x=$R0
  rt_f64_promote "$2"
  rt_f64_add "$x" "$R0"
  rt_f32_demote "$R0"
  return 0
}
