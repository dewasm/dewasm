// requires: memory/init
int wasi_random_get(int bufPtr, int length) {
    byte[] b = new byte[length];
    rng.nextBytes(b);
    memory.init(Integer.toUnsignedLong(bufPtr), b, 0, length);
    return WASI_OK;
}
