# A wasm trap: die with a blessed message carrier.
# Embedder evals can then tell traps from host perl errors.
sub trap {
    die bless({ message => $_[0] }, 'Rt::Trap');
}
