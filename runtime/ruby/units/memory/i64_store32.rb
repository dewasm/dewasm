def i64_store32(a, v) = (check(a, 4); @bytes[a, 4] = [v & M32].pack("L<"))
