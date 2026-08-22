# requires: rt/f32_lt
rt_f32_gt() {
  rt_f32_lt "$2" "$1"
  return $?
}
