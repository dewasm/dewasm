# requires: memory/i64_load8_u, rt/sext
def i64_load8_so(self, a, off):
    return Rt.sext(self.i64_load8_u(a + off), 8, Rt.M64)
