// requires: rt/trap
// Copy `length` funcrefs from `src` into the table at `dst` (active element
// segment at instantiation).
void init(int dst, Rt.Funcref[] src, int srcOff, int length) {
    long d = Integer.toUnsignedLong(dst);
    long s = Integer.toUnsignedLong(srcOff);
    long n = Integer.toUnsignedLong(length);
    if (d + n > slots.length || s + n > src.length) {
        Rt.trap("out of bounds table access");
    }
    System.arraycopy(src, (int) s, slots, (int) d, (int) n);
}
