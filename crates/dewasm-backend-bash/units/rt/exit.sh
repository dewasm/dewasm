# proc_exit protocol, the sibling of rt_trap's status 134.
# It sets the exit code and propagates status 133 through the `|| return $?` cascade.
# So a sourced module never kills the caller's shell.
EXIT_CODE=0
rt_exit() {
  EXIT_CODE=$1
  return 133
}
