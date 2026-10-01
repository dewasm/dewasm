# A wasm trap: die with a blessed message carrier.
# An embedder's `eval` can then tell traps from host Perl errors.
sub trap {
    die bless({ message => $_[0] }, 'Rt::Trap');
}
