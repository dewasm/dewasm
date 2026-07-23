wasi_fd_seek() {
  local -n __fds=${1}wfds
  if [[ -z ${__fds[$2]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  R0=70 # ESPIPE: only stdio exists and pipes cannot seek
  return 0
}
