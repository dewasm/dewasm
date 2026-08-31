// The trampoline a tail-calling function's entry runs.
// A tail call parks its target and arguments on the instance and returns; nothing is allocated per hop, which is what a chain of any length pays otherwise.
// A tail entry is bound to the instance that built it and reads *that* instance's parked slots, which is why `table/tail_slot` hands one back only to its owner.
interface TailBody {
	Object run();
}
