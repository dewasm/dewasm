# requires: rt/trap
def i32_loado(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.get_value(:u32, a))
