# requires: memory/udlho, rt/sext
def idlho(self, a, off):
    return Rt.sext(self.udlho(a, off), 16, Rt.M64)
