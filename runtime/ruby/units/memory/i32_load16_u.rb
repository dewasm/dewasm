# requires: rt/trap
def i32_load16_u(a) = (Rt.trap("out of bounds memory access") if a + 2 > @size; @buffer.get_value(:u16, a))
