// requires: memory/i32_store
func (w *WASI) wasi_args_sizes_get(argcPtr, bufSizePtr uint32) uint32 {
    total := uint32(0)
    for _, a := range w.args {
        total += uint32(len(a)) + 1
    }
    w.memory.i32_store(uint64(argcPtr), uint32(len(w.args)))
    w.memory.i32_store(uint64(bufSizePtr), total)
    return wasiOk
}
