// memory.fill: set `length` bytes at `addr` to the low byte of `value`.
void fill(long addr, long value, long length) {
    int a = at(addr, length);
    java.util.Arrays.fill(d, a, a + (int) length, (byte) value);
}
