// A uint32 identity that launders a folded i32 constant through a call.
// Go rejects int32(c) for a constant c above math.MaxInt32 at compile time, but a function-call result is never a constant, so a signed view like int32(Rt.i32c(0xffffffbf)) becomes a runtime reinterpretation.
// Used only for i32 constants that fold directly into a signed cast.
func (rt) i32c(v uint32) uint32 { return v }
