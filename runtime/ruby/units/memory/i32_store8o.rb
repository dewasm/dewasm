# requires: rt/trap
def i32_store8o(a, off, v) = (a += off; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.set_value(:U8, a, v & 0xff))
