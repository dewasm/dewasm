# requires: memory/uwlh, rt/sext
def iwlh(self, a):
    return Rt.sext(self.uwlh(a), 16, Rt.M32)
