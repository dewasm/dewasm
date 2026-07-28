# requires: rt/trap
# Fixed-arity `call_indirect` for a 7-argument signature (ADR-44): same
# dispatch and trap contract as `call`, but no `*args`/splat array is
# built on either the caller or the callee side.
def call7(i, type_key, a0, a1, a2, a3, a4, a5, a6)
  Rt.trap("undefined element") if i >= @slots.size
  slot = @slots[i]
  Rt.trap("uninitialized element") if slot.nil?
  ty, func = slot
  Rt.trap("indirect call type mismatch") unless ty == type_key
  func.call(a0, a1, a2, a3, a4, a5, a6)
end
