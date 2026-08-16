# requires: memory/uwlbo, rt/sext
def iwlbo(self, a, off):
    return Rt.sext(self.uwlbo(a, off), 8, Rt.M32)
