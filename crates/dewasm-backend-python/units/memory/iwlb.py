# requires: memory/uwlb, rt/sext
def iwlb(self, a):
    return Rt.sext(self.uwlb(a), 8, Rt.M32)
