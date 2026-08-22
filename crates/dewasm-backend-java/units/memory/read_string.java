// requires: rt/trap
byte[] read_string(long addr, long length) {
    int a = at(addr, length);
    byte[] out = new byte[(int) length];
    System.arraycopy(d, a, out, 0, (int) length);
    return out;
}
