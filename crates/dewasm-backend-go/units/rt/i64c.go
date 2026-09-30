// A uint64 identity that launders a folded i64 constant through a call.
// So a signed view like int64(Rt.i64c(v)) reinterprets at runtime.
// It does not trip Go's compile-time "constant overflows int64" check.
// The i64 counterpart of `i32c`.
func (rt) i64c(v uint64) uint64 { return v }
