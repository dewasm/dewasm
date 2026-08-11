// requires: rt/trap
// Linear memory: a byte[] with a little-endian ByteBuffer view, plus a page
// cap. Effective addresses are computed as unsigned longs by callers (guest
// addr + offset can exceed 2^31), so every method takes a long address and the
// bounds check is exact. The ByteBuffer is re-wrapped on grow.
byte[] d;
java.nio.ByteBuffer bb;
int maxPages;

Memory(int minPages, int maxPages) {
    // A spec-legal minimum of 32768+ pages (2 GiB+) exceeds what a Java
    // byte[] can hold; fail instantiation with a clear trap instead of the
    // NegativeArraySizeException the overflowing int multiply would throw.
    long bytes = (long) minPages * 65536;
    if (bytes > Integer.MAX_VALUE) {
        Rt.trap("cannot allocate " + minPages + " pages of linear memory (exceeds Java's byte[] limit)");
    }
    this.d = new byte[(int) bytes];
    this.maxPages = maxPages;
    rewrap();
}

private void rewrap() {
    this.bb = java.nio.ByteBuffer.wrap(d).order(java.nio.ByteOrder.LITTLE_ENDIAN);
}

// Bounds-check [addr, addr+length) and return the int index into the buffer.
private int at(long addr, long length) {
    if (addr < 0 || addr + length > d.length) {
        Rt.trap("out of bounds memory access");
    }
    return (int) addr;
}
