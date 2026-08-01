# The interpreter gate (ADR-15): the generated numerics assume 64-bit IVs
# and IEEE-double NVs; die loudly on any other perl build rather than
# silently mis-compute.
use Config ();
die "dewasm: this program requires a perl built with 64-bit integers and doubles (ivsize=8, nvsize=8); this perl has ivsize=$Config::Config{ivsize}, nvsize=$Config::Config{nvsize}\n"
    unless $Config::Config{ivsize} == 8 && $Config::Config{nvsize} == 8;

# Explicit call-depth accounting (ADR-55): perl recursion grows on the heap
# and is only cut off by the OOM killer, so runaway guest recursion must be
# stopped by accounting for `call stack exhausted` to trap deterministically.
# The unit is frame-size slots, not calls: each generated function adds
# 1 + (params + locals + temps) / 8, approximating the byte-bounded native
# stack so fat-frame recursion traps early instead of hoarding heap.
our $DEPTH = 0;
our $LIMIT = 100000;
