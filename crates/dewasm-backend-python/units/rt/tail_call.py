# A pending tail call: a tail-calling function's body returns one of these instead of calling, and its entry wrapper unwraps them in a loop, so a chain of any length runs in constant stack space.
# A plain list, recognized by its type: a wasm result is a number, a bound method (funcref), or a tuple (multi-value), never a list, so nothing else can look like one.
@staticmethod
def tail_call(target, args):
    return [target, args]

@staticmethod
def trampoline(r):
    while type(r) is list:
        r = r[0](*r[1])
    return r
