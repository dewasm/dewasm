# requires: rt/exit
wasi_proc_exit() {
  rt_exit "$2"
  return $?
}
