# requires: rt/f32_le
rt_f32_ge() {
  rt_f32_le "$2" "$1"
  return $?
}
