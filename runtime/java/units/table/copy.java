// requires: rt/trap
// table.copy: move `length` funcref slots from `other` (possibly this table)
// at `src` to this table at `dst`. Both ranges are bounds-checked before any
// write; System.arraycopy handles overlap like memmove (ADR-16).
void copy(int dst, Table other, int src, int length) {
    long d = Integer.toUnsignedLong(dst);
    long s = Integer.toUnsignedLong(src);
    long n = Integer.toUnsignedLong(length);
    if (s + n > other.slots.length || d + n > slots.length) {
        Rt.trap("out of bounds table access");
    }
    if (n == 0) {
        return;
    }
    System.arraycopy(other.slots, (int) s, slots, (int) d, (int) n);
}
