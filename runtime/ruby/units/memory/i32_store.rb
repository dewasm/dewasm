def i32_store(a, v) = (check(a, 4); @bytes[a, 4] = [v].pack("L<"))
