# `proc_exit` protocol, the counterpart of `rt_trap`'s status 134.
# It sets the exit code and passes status 133 up the `|| return $?` chain.
# So a sourced module never kills the caller's shell.
EXIT_CODE=0
rt_exit() {
  EXIT_CODE=$1
  return 133
}
