# requires: memory/uwlho, rt/sext
def iwlho(self, a, off):
    return Rt.sext(self.uwlho(a, off), 16, Rt.M32)
