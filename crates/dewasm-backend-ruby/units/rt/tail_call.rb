# The trampoline a tail-calling function's entry runs.
# A tail call parks its target, its arity and its arguments and returns; nothing is allocated per hop, which is what a chain of any length pays otherwise.
# The target is cleared before dispatching, so a callee that does not tail-call ends the chain, and the arguments are read as the call's own operands, before the callee can overwrite them.
# Fixed-arity dispatch for the same reason `table/call<n>` has it: a splat would build an array on both sides of every hop. Arities past the fixed set park an array instead (`@__taw`).
def trampoline(r)
  while (f = @__tf)
    @__tf = nil
    r = case @__tn
        when 0 then f.call
        when 1 then f.call(@__ta0)
        when 2 then f.call(@__ta0, @__ta1)
        when 3 then f.call(@__ta0, @__ta1, @__ta2)
        when 4 then f.call(@__ta0, @__ta1, @__ta2, @__ta3)
        when 5 then f.call(@__ta0, @__ta1, @__ta2, @__ta3, @__ta4)
        when 6 then f.call(@__ta0, @__ta1, @__ta2, @__ta3, @__ta4, @__ta5)
        when 7 then f.call(@__ta0, @__ta1, @__ta2, @__ta3, @__ta4, @__ta5, @__ta6)
        when 8 then f.call(@__ta0, @__ta1, @__ta2, @__ta3, @__ta4, @__ta5, @__ta6, @__ta7)
        else f.call(*@__taw)
        end
  end
  r
end
