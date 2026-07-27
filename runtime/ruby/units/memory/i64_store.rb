# requires: rt/trap
def i64_store(a, v) = (Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.set_value(:u64, a, v))
