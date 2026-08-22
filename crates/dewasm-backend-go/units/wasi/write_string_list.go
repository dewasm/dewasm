// requires: memory/i32_store, memory/i32_store8, memory/init
func (w *WASI) write_string_list(strings [][]byte, listPtr, bufPtr uint32) uint32 {
    for i, s := range strings {
        w.memory.i32_store(uint64(listPtr)+uint64(i)*4, bufPtr)
        w.memory.init(uint64(bufPtr), s, 0, uint64(len(s)))
        w.memory.i32_store8(uint64(bufPtr)+uint64(len(s)), 0)
        bufPtr += uint32(len(s)) + 1
    }
    return wasiOk
}
