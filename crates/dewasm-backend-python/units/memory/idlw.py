# requires: memory/udlw, rt/sext
def idlw(self, a):
    return Rt.sext(self.udlw(a), 32, Rt.M64)
