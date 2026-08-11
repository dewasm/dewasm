// requires: rt/trap
// Also used to initialize active data segments at instantiation time, and to copy a WASI buffer in.
void init(long addr, byte[] src, long srcOff, long length) {
    int a = at(addr, length);
    if (srcOff < 0 || srcOff + length > src.length) {
        Rt.trap("out of bounds memory access");
    }
    System.arraycopy(src, (int) srcOff, d, a, (int) length);
}
