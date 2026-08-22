# requires: memory/udlba, rt/sext
def idlba(self, a, b):
    return Rt.sext(self.udlba(a, b), 8, Rt.M64)
