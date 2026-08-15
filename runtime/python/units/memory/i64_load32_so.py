# requires: memory/i64_load32_u, rt/sext
def i64_load32_so(self, a, off):
    return Rt.sext(self.i64_load32_u(a + off), 32, Rt.M64)
