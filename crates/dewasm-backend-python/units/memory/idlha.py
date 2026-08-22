# requires: memory/udlha, rt/sext
def idlha(self, a, b):
    return Rt.sext(self.udlha(a, b), 16, Rt.M64)
