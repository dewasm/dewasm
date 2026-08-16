# requires: memory/i64_load16_ua, rt/sext
def i64_load16_sa(self, a, b):
    return Rt.sext(self.i64_load16_ua(a, b), 16, Rt.M64)
