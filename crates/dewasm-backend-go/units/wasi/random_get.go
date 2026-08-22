// requires: memory/init
func (w *WASI) wasi_random_get(bufPtr, length uint32) uint32 {
    b := make([]byte, length)
    if _, err := rand.Read(b); err != nil {
        return wasiIo
    }
    w.memory.init(uint64(bufPtr), b, 0, uint64(length))
    return wasiOk
}
