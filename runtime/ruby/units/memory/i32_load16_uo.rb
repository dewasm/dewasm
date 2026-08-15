# requires: rt/trap
def i32_load16_uo(a, off) = (a += off; Rt.trap("out of bounds memory access") if a + 2 > @size; @buffer.get_value(:u16, a))
