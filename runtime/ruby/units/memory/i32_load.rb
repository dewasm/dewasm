# requires: rt/trap
def i32_load(a) = (a &= M32; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.get_value(:u32, a))
