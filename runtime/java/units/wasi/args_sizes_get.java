// requires: memory/i32_store
int wasi_args_sizes_get(int argcPtr, int bufSizePtr) {
    int total = 0;
    for (byte[] a : args) {
        total += a.length + 1;
    }
    memory.i32_store(Integer.toUnsignedLong(argcPtr), args.length);
    memory.i32_store(Integer.toUnsignedLong(bufSizePtr), total);
    return WASI_OK;
}
