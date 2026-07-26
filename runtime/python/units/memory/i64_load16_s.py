# requires: memory/i64_load16_u, rt/sext
def i64_load16_s(self, a):
    return Rt.sext(self.i64_load16_u(a), 16, Rt.M64)
