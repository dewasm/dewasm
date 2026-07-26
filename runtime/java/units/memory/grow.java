// memory.grow: extend by `delta` pages (delta read unsigned); returns the old
// size in pages, or -1 (i32 0xFFFFFFFF) if the page cap would be exceeded.
int grow(int delta) {
    int old = d.length / 65536;
    long want = (long) old + (delta & 0xFFFFFFFFL);
    if (want > maxPages) {
        return -1;
    }
    byte[] nd = new byte[(int) (want * 65536)];
    System.arraycopy(d, 0, nd, 0, d.length);
    d = nd;
    rewrap();
    return old;
}
