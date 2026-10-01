// requires: rt/trap
// Linear memory: a byte[] with a little-endian ByteBuffer view, plus a page cap.
// Callers compute effective addresses as unsigned longs, since `addr + offset` can exceed 2^31.
// So every method takes a long address, and the bounds check is exact.
// The ByteBuffer is re-wrapped on grow.
byte[] d;
java.nio.ByteBuffer bb;
// Visible memory size in bytes.
// `d.length` is capacity and may exceed it (grow reallocates geometrically).
int size;
int maxPages;

Memory(int minPages, int maxPages) {
    // A minimum of 32768+ pages (2 GiB+) is valid wasm but exceeds what a Java byte[] can hold.
    // So fail instantiation with a clear trap.
    // The overflowing `int` multiply would otherwise throw NegativeArraySizeException.
    long bytes = (long) minPages * 65536;
    if (bytes > Integer.MAX_VALUE) {
        Rt.trap("cannot allocate " + minPages + " pages of linear memory (exceeds Java's byte[] limit)");
    }
    this.d = new byte[(int) bytes];
    this.size = (int) bytes;
    this.maxPages = maxPages;
    rewrap();
}

private void rewrap() {
    this.bb = java.nio.ByteBuffer.wrap(d).order(java.nio.ByteOrder.LITTLE_ENDIAN);
}

// Bounds-check `[addr, addr+length)` and return the `int` index into the buffer.
// The check is against size, not d.length: the tail past size is unreachable.
// That tail is never written, so it stays zero-filled until grow makes it visible.
// Java zero-initializes arrays.
private int at(long addr, long length) {
    if (addr < 0 || addr + length > size) {
        Rt.trap("out of bounds memory access");
    }
    return (int) addr;
}
