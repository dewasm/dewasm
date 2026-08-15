# requires: rt/trap, rt/m64
def i64_store(a, v) = (a &= M32; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.set_value(:u64, a, Rt.m64(v)))
