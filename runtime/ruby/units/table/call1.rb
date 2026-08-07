# requires: rt/trap
# Fixed-arity `call_indirect` for a 1-argument signature: same
# dispatch and trap contract as `call`, but no `*args`/splat array is
# built on either the caller or the callee side.
def call1(i, type_key, a0)
  Rt.trap("undefined element") if i >= @slots.size
  slot = @slots[i]
  Rt.trap("uninitialized element") if slot.nil?
  ty, func = slot
  Rt.trap("indirect call type mismatch") unless ty == type_key
  func.call(a0)
end
