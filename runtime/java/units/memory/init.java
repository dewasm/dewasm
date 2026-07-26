// requires: rt/trap
// Copy `length` bytes from a source blob (a data segment, or a WASI buffer)
// into memory at `addr`. Bounds-checked on both sides.
void init(long addr, byte[] src, long srcOff, long length) {
    int a = at(addr, length);
    if (srcOff < 0 || srcOff + length > src.length) {
        Rt.trap("out of bounds memory access");
    }
    System.arraycopy(src, (int) srcOff, d, a, (int) length);
}
