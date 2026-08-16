# requires: memory/i32_load8_uo, rt/sext
def i32_load8_so(self, a, off):
    return Rt.sext(self.i32_load8_uo(a, off), 8, Rt.M32)
