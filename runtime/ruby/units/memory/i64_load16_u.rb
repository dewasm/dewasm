def i64_load16_u(a) = (check(a, 2); @bytes.unpack1("S<", offset: a))
