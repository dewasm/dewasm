# requires: rt/trap
def i32_load8_s(a) = (a &= M32; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.get_value(:S8, a) & M32)
