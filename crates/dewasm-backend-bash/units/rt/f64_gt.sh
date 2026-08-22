# requires: rt/f64_lt
rt_f64_gt() {
  rt_f64_lt "$2" "$1"
  return $?
}
