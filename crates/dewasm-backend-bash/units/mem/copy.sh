# requires: mem/check
# Direction-aware byte loop so overlapping ranges copy correctly.
mem_copy() {
  local -n __m=${1}mem
  local d=$2 s=$3 n=$4 i dk sk
  mem_check "$1" "$d" "$n" || return $?
  mem_check "$1" "$s" "$n" || return $?
  if (( d <= s )); then
    for (( i = 0; i < n; i++ )); do
      dk=$(( d + i )); sk=$(( s + i ))
      __m[$dk]=$(( __m[$sk] ))
    done
  else
    for (( i = n - 1; i >= 0; i-- )); do
      dk=$(( d + i )); sk=$(( s + i ))
      __m[$dk]=$(( __m[$sk] ))
    done
  fi
  return 0
}
