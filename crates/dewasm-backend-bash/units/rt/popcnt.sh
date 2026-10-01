# Shared by i32.popcnt and i64.popcnt.
# i32 values are non-negative, and i64 bit patterns count and clear their sign bit first.
# So the loop always ends.
rt_popcnt() {
  local x=$1 n=0
  if (( x < 0 )); then
    (( n = 1, x &= 0x7fffffffffffffff ))
  fi
  while (( x != 0 )); do
    (( n += x & 1, x >>= 1 ))
  done
  R0=$n
  return 0
}
