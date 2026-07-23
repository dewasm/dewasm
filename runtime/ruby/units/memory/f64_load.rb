def f64_load(a) = (check(a, 8); @bytes.unpack1("E", offset: a))
