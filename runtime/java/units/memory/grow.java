// memory.grow: extend by `delta` pages (delta read unsigned); returns the old
// size in pages, or -1 (i32 0xFFFFFFFF) if the request cannot be satisfied.
// The byte size is computed in long and checked against the byte[] cap before
// the int cast (32768 pages is already 2^31 bytes, one past what a Java array
// can hold), and an allocation failure is likewise reported as -1 — wasm lets
// grow fail, never crash.
int grow(int delta) {
    int old = d.length / 65536;
    long want = (long) old + (delta & 0xFFFFFFFFL);
    if (want > maxPages || want * 65536 > Integer.MAX_VALUE) {
        return -1;
    }
    byte[] nd;
    try {
        nd = new byte[(int) (want * 65536)];
    } catch (OutOfMemoryError e) {
        return -1;
    }
    System.arraycopy(d, 0, nd, 0, d.length);
    d = nd;
    rewrap();
    return old;
}
