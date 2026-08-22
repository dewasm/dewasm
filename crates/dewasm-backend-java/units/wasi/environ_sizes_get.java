// requires: memory/i32_store
int wasi_environ_sizes_get(int countPtr, int bufSizePtr) {
    int total = 0;
    for (byte[] e : env) {
        total += e.length + 1;
    }
    memory.i32_store(Integer.toUnsignedLong(countPtr), env.length);
    memory.i32_store(Integer.toUnsignedLong(bufSizePtr), total);
    return WASI_OK;
}
