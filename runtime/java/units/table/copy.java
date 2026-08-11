// requires: rt/trap
// Both ranges are bounds-checked before any write, and `other` may be this table: System.arraycopy handles overlap like memmove.
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
