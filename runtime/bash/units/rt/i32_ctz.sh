rt_i32_ctz() {
  local x=$1 n=0
  if (( x == 0 )); then
    R0=32
    return 0
  fi
  while (( (x & 1) == 0 )); do
    (( n += 1, x >>= 1 ))
  done
  R0=$n
  return 0
}
