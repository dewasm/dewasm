// requires: wasi/write_string_list
func (w *WASI) wasi_args_get(argvPtr, bufPtr uint32) uint32 {
    return w.write_string_list(w.args, argvPtr, bufPtr)
}
