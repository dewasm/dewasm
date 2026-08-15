# requires: rt/trap
def i64_load(a) = (a &= M32; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.get_value(:u64, a))
