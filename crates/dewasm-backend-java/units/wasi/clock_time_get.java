// requires: memory/i64_store
int wasi_clock_time_get(int id, long precision, int outPtr) {
    switch (id) {
        case 0: // realtime
        case 1: // monotonic
        case 2: // process cputime
        case 3: // thread cputime
            memory.i64_store(
                Integer.toUnsignedLong(outPtr), System.currentTimeMillis() * 1_000_000L);
            return WASI_OK;
        default:
            return WASI_INVAL;
    }
}
