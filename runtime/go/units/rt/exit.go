// A WASI proc_exit request, distinct from a trap: carries the exit code up to
// the public boundary.
type rtExit struct{ code int }

func (rt) exit(code int) {
    panic(&rtExit{code})
}
