# requires: rt/f64_add
# a - b = a + (-b), exact for every case including the NaN and signed-zero tables (-(−0) = +0).
rt_f64_sub() {
  rt_f64_add "$1" $(( $2 ^ (1 << 63) ))
  return $?
}
