# requires: rt/trap
# Same dispatch and trap contract as `call`, with no `*args`/splat array built on either side.
def call1(i, type_key, a0)
  Rt.trap("undefined element") if i >= @slots.size
  slot = @slots[i]
  Rt.trap("uninitialized element") if slot.nil?
  ty, func = slot
  Rt.trap("indirect call type mismatch") unless ty == type_key
  func.call(a0)
end
