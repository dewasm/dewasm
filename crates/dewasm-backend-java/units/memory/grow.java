// memory.grow: returns the old size in pages, or -1 when unsatisfiable.
// Byte size checked in long against the byte[] cap (32768 pages > max array);
// allocation failure is also -1: wasm lets grow fail, never crash.
int grow(int delta) {
    int old = size / 65536;
    long want = (long) old + (delta & 0xFFFFFFFFL);
    if (want > maxPages || want * 65536 > Integer.MAX_VALUE) {
        return -1;
    }
    int newSize = (int) (want * 65536);
    if (newSize > d.length) {
        // Geometric capacity amortizes one-page grow loops; the new array is zero-initialized, so the pages becoming visible are zero (see at).
        long cap = Math.min((long) maxPages * 65536, Integer.MAX_VALUE);
        byte[] nd;
        try {
            nd = new byte[(int) Math.max(newSize, Math.min((long) d.length * 2, cap))];
        } catch (OutOfMemoryError e) {
            return -1;
        }
        System.arraycopy(d, 0, nd, 0, size);
        d = nd;
        rewrap();
    }
    size = newSize;
    return old;
}
