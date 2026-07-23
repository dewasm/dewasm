def i32_load(a) = (check(a, 4); @bytes.unpack1("L<", offset: a))
