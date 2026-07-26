// A failed import resolution at instantiation time (missing import, or one of
// the wrong Go type), kept distinct from a trap.
type rtLinkError struct{ msg string }

func (e *rtLinkError) Error() string { return e.msg }

func (rt) link_error(msg string) {
    panic(&rtLinkError{msg})
}
