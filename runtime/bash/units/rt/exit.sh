# proc_exit protocol, the sibling of rt_trap's status 134: set
# the exit code and propagate status 133 through the `|| return $?`
# cascade so a sourced module never kills the caller's shell.
EXIT_CODE=0
rt_exit() {
  EXIT_CODE=$1
  return 133
}
