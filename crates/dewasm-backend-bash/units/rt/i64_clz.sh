rt_i64_clz() {
  local x=$1 n=0
  if (( x == 0 )); then
    R0=64
    return 0
  fi
  while (( x > 0 )); do
    (( n += 1, x <<= 1 ))
  done
  R0=$n
  return 0
}
