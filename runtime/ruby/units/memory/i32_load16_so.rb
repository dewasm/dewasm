# requires: rt/trap
def i32_load16_so(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 2 > @size; @buffer.get_value(:s16, a) & M32)
