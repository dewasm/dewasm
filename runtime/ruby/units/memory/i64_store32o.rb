# requires: rt/trap
def i64_store32o(a, off, v) = (a += off; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.set_value(:u32, a, v & M32))
