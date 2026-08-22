long i64_load32_u(long addr) { return bb.getInt(at(addr, 4)) & 0xFFFFFFFFL; }
