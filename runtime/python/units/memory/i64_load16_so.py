# requires: memory/i64_load16_uo, rt/sext
def i64_load16_so(self, a, off):
    return Rt.sext(self.i64_load16_uo(a, off), 16, Rt.M64)
