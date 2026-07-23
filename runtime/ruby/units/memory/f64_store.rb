def f64_store(a, v) = (check(a, 8); @bytes[a, 8] = [v].pack("E"))
