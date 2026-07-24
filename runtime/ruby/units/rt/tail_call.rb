# A pending tail call (ADR-18): entry wrappers unwrap these in a loop so
# tail-call chains run in constant stack space.
TailCall = Struct.new(:target, :args)

def trampoline(r)
  r = r.target.call(*r.args) while r.is_a?(TailCall)
  r
end
