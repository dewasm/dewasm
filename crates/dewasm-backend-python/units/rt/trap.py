class Trap(Exception):
    pass

@staticmethod
def trap(message):
    raise Rt.Trap(message)
