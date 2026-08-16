# requires: memory/uwlba, rt/sext
def iwlba(self, a, b):
    return Rt.sext(self.uwlba(a, b), 8, Rt.M32)
