// requires: wasi/write_string_list
func (w *WASI) wasi_environ_get(environPtr, bufPtr uint32) uint32 {
    return w.write_string_list(w.env, environPtr, bufPtr)
}
