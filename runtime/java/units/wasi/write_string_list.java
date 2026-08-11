// requires: memory/i32_store, memory/i32_store8, memory/init
// The WASI two-part layout shared by args_get/environ_get: a pointer array plus
// NUL-terminated bytes in a separate buffer.
int write_string_list(byte[][] xs, int listPtr, int bufPtr) {
    long lp = Integer.toUnsignedLong(listPtr);
    long bp = Integer.toUnsignedLong(bufPtr);
    for (byte[] s : xs) {
        memory.i32_store(lp, (int) bp);
        memory.init(bp, s, 0, s.length);
        memory.i32_store8(bp + s.length, 0);
        lp += 4;
        bp += s.length + 1;
    }
    return WASI_OK;
}
