# The trampoline a tail-calling function's entry runs.
# A tail call parks its target, its arity and its arguments on the instance and returns; nothing is allocated per hop, which is what a chain of any length pays otherwise.
# The target is cleared before dispatching, so a callee that does not tail-call ends the chain, and the arguments are read as the call's own operands, before the callee can overwrite them.
# Fixed-arity dispatch for the same reason `table/call` avoids a splat: unpacking would build a tuple on both sides of every hop. Arities past the fixed set park a tuple instead (`_taw`).
@staticmethod
def trampoline(inst, r):
    while True:
        f = inst._tf
        if f is None:
            return r
        inst._tf = None
        n = inst._tn
        if n == 1:
            r = f(inst._ta0)
        elif n == 2:
            r = f(inst._ta0, inst._ta1)
        elif n == 3:
            r = f(inst._ta0, inst._ta1, inst._ta2)
        elif n == 0:
            r = f()
        elif n == 4:
            r = f(inst._ta0, inst._ta1, inst._ta2, inst._ta3)
        elif n == 5:
            r = f(inst._ta0, inst._ta1, inst._ta2, inst._ta3, inst._ta4)
        elif n == 6:
            r = f(inst._ta0, inst._ta1, inst._ta2, inst._ta3, inst._ta4, inst._ta5)
        elif n == 7:
            r = f(inst._ta0, inst._ta1, inst._ta2, inst._ta3, inst._ta4, inst._ta5, inst._ta6)
        elif n == 8:
            r = f(inst._ta0, inst._ta1, inst._ta2, inst._ta3, inst._ta4, inst._ta5, inst._ta6, inst._ta7)
        else:
            r = f(*inst._taw)
