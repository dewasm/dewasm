# requires: memory/udlwa, rt/sext
def idlwa(self, a, b):
    return Rt.sext(self.udlwa(a, b), 32, Rt.M64)
