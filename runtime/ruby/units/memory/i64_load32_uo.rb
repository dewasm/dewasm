# requires: rt/trap
def i64_load32_uo(a, off) = (a += off; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.get_value(:u32, a))
