def i64_store8(a, v) = (check(a, 1); @bytes.setbyte(a, v & 0xff))
