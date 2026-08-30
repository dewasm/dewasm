// A pending tail call: a tail-calling function's body returns one of these instead of calling, and its entry wrapper unwraps them in a loop, so a chain of any length runs in constant stack space.
// A wasm result is a boxed primitive, an `Object[]` (multi-value), or an `Fn` (funcref), never one of these, so nothing else can look like one.
interface TailThunk {
	Object run();
}

static final class TailCall {
	final TailThunk thunk;

	TailCall(TailThunk thunk) {
		this.thunk = thunk;
	}
}

static Object trampoline(Object r) {
	while (r instanceof TailCall) {
		r = ((TailCall) r).thunk.run();
	}
	return r;
}
