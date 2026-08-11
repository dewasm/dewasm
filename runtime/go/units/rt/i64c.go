// A uint64 identity that launders a folded i64 constant through a call, so a signed view like int64(Rt.i64c(v)) reinterprets at runtime instead of tripping Go's compile-time "constant overflows int64" check.
// The i64 analog of i32c.
func (rt) i64c(v uint64) uint64 { return v }
