# requires: mem/check
# WASI fd_prestat_dir_name: write a preopen dir fd's guest-visible name into guest memory.
# A name longer than the caller's buffer is ENAMETOOLONG (37), matching Ruby (runtime/ruby/units/wasi/fd_prestat_dir_name.rb).
# Only a preopen (dir fd with <p>wname set) has a name; anything else is EBADF (8).
wasi_fd_prestat_dir_name() {
  local __p=$1 __fd=$2 __path_ptr=$3 __path_len=$4
  local -n __m=${__p}mem
  local -n __fds=${__p}wfds
  local -n __wname=${__p}wname
  if [[ ${__fds[$__fd]-} != 3 || -z ${__wname[$__fd]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  local LC_ALL=C
  local __name=${__wname[$__fd]}
  local __n=${#__name}
  if (( __n > __path_len )); then
    R0=37 # ENAMETOOLONG
    return 0
  fi
  mem_check "$__p" "$__path_ptr" "$__n" || return $?
  local __i __b __mk
  for (( __i = 0; __i < __n; __i++ )); do
    printf -v __b '%d' "'${__name:__i:1}"
    __mk=$(( __path_ptr + __i ))
    __m[$__mk]=$(( __b & 0xff ))
  done
  R0=0
  return 0
}
