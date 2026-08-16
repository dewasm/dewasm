# requires: memory/i64_load8_ua, rt/sext
def i64_load8_sa(self, a, b):
    return Rt.sext(self.i64_load8_ua(a, b), 8, Rt.M64)
