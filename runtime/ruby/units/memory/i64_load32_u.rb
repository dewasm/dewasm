def i64_load32_u(a) = (check(a, 4); @bytes.unpack1("L<", offset: a))
