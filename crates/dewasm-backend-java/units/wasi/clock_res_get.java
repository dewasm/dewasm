// requires: memory/i64_store
int wasi_clock_res_get(int id, int outPtr) {
    memory.i64_store(Integer.toUnsignedLong(outPtr), 1);
    return WASI_OK;
}
