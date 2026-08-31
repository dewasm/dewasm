# A pending tail call: a tail-calling function's body returns one of these instead of calling, and its entry wrapper unwraps them in a loop, so a chain of any length runs in constant stack space.
TailCall = Struct.new(:target, :args)

def trampoline(r)
  r = r.target.call(*r.args) while r.is_a?(TailCall)
  r
end
