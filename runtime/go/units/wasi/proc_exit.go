// requires: rt/exit
func (w *WASI) wasi_proc_exit(code uint32) {
    Rt.exit(int(code))
}
