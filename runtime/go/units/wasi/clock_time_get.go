// requires: memory/i64_store
func (w *WASI) wasi_clock_time_get(id uint32, precision uint64, outPtr uint32) uint32 {
    switch id {
    case 0, 1, 2, 3: // realtime / monotonic / process / thread cputime
        w.memory.i64_store(uint64(outPtr), uint64(time.Now().UnixNano()))
    default:
        return wasiInval
    }
    return wasiOk
}
