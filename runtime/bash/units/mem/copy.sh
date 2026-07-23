# requires: mem/check
# Direction-aware byte loop so overlapping ranges copy correctly.
mem_copy() {
  local -n __m=${1}mem
  local d=$2 s=$3 n=$4 i
  mem_check "$1" "$d" "$n" || return $?
  mem_check "$1" "$s" "$n" || return $?
  if (( d <= s )); then
    for (( i = 0; i < n; i++ )); do
      __m[d+i]=$(( __m[s+i] ))
    done
  else
    for (( i = n - 1; i >= 0; i-- )); do
      __m[d+i]=$(( __m[s+i] ))
    done
  fi
  return 0
}
