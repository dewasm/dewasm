# requires: memory/udlb, rt/sext
def idlb(self, a):
    return Rt.sext(self.udlb(a), 8, Rt.M64)
