def i64_load(a) = (check(a, 8); @bytes.unpack1("Q<", offset: a))
