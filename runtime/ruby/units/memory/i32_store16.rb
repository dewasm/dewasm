def i32_store16(a, v) = (check(a, 2); @bytes[a, 2] = [v & 0xffff].pack("S<"))
