// requires: memory/i64_store
func (w *WASI) wasi_clock_res_get(id, outPtr uint32) uint32 {
    w.memory.i64_store(uint64(outPtr), 1)
    return wasiOk
}
