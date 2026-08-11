// memory.copy must be overlap-safe; System.arraycopy is, like memmove.
void copy(long dst, long src, long length) {
    int da = at(dst, length);
    int sa = at(src, length);
    System.arraycopy(d, sa, d, da, (int) length);
}
