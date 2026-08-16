# requires: memory/i64_load32_ua, rt/sext
def i64_load32_sa(self, a, b):
    return Rt.sext(self.i64_load32_ua(a, b), 32, Rt.M64)
