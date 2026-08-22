# requires: rt/f32_eq
rt_f32_ne() {
  rt_f32_eq "$1" "$2"
  R0=$(( 1 - R0 ))
  return 0
}
