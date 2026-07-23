wasi_fd_close() {
  local -n __fds=${1}wfds
  if [[ -z ${__fds[$2]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  unset "__fds[$2]"
  R0=0
  return 0
}
