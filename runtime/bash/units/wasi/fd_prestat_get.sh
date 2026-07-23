# No preopens: EBADF is what stops a libc's preopen scan.
wasi_fd_prestat_get() {
  R0=8 # EBADF
  return 0
}
