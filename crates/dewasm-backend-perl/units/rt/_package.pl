# The interpreter version check: the generated numerics assume 64-bit IVs and IEEE-double NVs.
# Die loudly on any other perl build rather than silently mis-compute.
use Config ();
die "dewasm: this program requires a perl built with 64-bit integers and doubles (ivsize=8, nvsize=8); this perl has ivsize=$Config::Config{ivsize}, nvsize=$Config::Config{nvsize}\n"
    unless $Config::Config{ivsize} == 8 && $Config::Config{nvsize} == 8;

# Explicit call-depth accounting.
# Perl recursion grows on the heap, and only the OOM killer cuts it off.
# So runaway guest recursion must be stopped by accounting.
# `call stack exhausted` then traps deterministically.
# The unit is frame-size slots, not calls.
# Each generated function adds 1 + (params + locals + temps) / 8.
# This approximates the byte-bounded native stack.
# Fat-frame recursion then traps early instead of hoarding heap.
our $DEPTH = 0;
our $LIMIT = 100000;
