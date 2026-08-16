# requires: rt/trap
def i32_loada(a, b) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.get_value(:u32, a))
