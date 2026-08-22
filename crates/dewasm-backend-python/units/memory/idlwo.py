# requires: memory/udlwo, rt/sext
def idlwo(self, a, off):
    return Rt.sext(self.udlwo(a, off), 32, Rt.M64)
