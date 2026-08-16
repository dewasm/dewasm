# requires: memory/uwlha, rt/sext
def iwlha(self, a, b):
    return Rt.sext(self.uwlha(a, b), 16, Rt.M32)
