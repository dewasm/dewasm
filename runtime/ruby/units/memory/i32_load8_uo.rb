# requires: rt/trap
def i32_load8_uo(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.get_value(:U8, a))
