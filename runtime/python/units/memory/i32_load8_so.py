# requires: memory/i32_load8_u, rt/sext
def i32_load8_so(self, a, off):
    return Rt.sext(self.i32_load8_u(a + off), 8, Rt.M32)
