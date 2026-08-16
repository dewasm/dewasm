# requires: memory/udlbo, rt/sext
def idlbo(self, a, off):
    return Rt.sext(self.udlbo(a, off), 8, Rt.M64)
