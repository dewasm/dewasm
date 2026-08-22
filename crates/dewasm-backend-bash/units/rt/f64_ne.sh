# requires: rt/f64_eq
rt_f64_ne() {
  rt_f64_eq "$1" "$2"
  R0=$(( 1 - R0 ))
  return 0
}
