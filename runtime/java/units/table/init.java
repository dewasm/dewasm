// requires: rt/trap
// Also used to initialize active element segments at instantiation time.
void init(int dst, Rt.Funcref[] src, int srcOff, int length) {
    long d = Integer.toUnsignedLong(dst);
    long s = Integer.toUnsignedLong(srcOff);
    long n = Integer.toUnsignedLong(length);
    if (d + n > slots.length || s + n > src.length) {
        Rt.trap("out of bounds table access");
    }
    System.arraycopy(src, (int) s, slots, (int) d, (int) n);
}
