# requires: rt/trap
def i64_load(a) = (Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.get_value(:u64, a))
