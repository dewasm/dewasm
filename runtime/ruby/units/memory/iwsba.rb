# requires: rt/trap
def iwsba(a, b, v) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.set_value(:U8, a, v & 0xff))
