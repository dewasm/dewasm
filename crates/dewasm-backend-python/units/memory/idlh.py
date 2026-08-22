# requires: memory/udlh, rt/sext
def idlh(self, a):
    return Rt.sext(self.udlh(a), 16, Rt.M64)
