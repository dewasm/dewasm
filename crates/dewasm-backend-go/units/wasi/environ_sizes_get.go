// requires: memory/i32_store
func (w *WASI) wasi_environ_sizes_get(countPtr, bufSizePtr uint32) uint32 {
    total := uint32(0)
    for _, e := range w.env {
        total += uint32(len(e)) + 1
    }
    w.memory.i32_store(uint64(countPtr), uint32(len(w.env)))
    w.memory.i32_store(uint64(bufSizePtr), total)
    return wasiOk
}
