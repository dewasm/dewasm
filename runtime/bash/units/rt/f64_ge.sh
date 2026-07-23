# requires: rt/f64_le
rt_f64_ge() {
  rt_f64_le "$2" "$1"
  return $?
}
