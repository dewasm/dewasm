# requires: rt/trap
def i32_load8_u(a) = (Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.get_value(:U8, a))
