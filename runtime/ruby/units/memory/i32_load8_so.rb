# requires: rt/trap
def i32_load8_so(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.get_value(:S8, a) & M32)
