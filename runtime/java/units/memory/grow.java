// memory.grow: returns the old size in pages, or -1 when unsatisfiable.
// Byte size checked in long against the byte[] cap (32768 pages > max array);
// allocation failure is also -1 — wasm lets grow fail, never crash.
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
