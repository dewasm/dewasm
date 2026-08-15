# requires: memory/i32_load16_uo, rt/sext
def i32_load16_so(self, a, off):
    return Rt.sext(self.i32_load16_uo(a, off), 16, Rt.M32)
