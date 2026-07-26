// memory.copy: overlapping-safe copy of `length` bytes from `src` to `dst`.
void copy(long dst, long src, long length) {
    int da = at(dst, length);
    int sa = at(src, length);
    System.arraycopy(d, sa, d, da, (int) length);
}
