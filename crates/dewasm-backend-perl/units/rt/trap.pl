# A wasm trap: die with a blessed message carrier so embedder evals can tell traps from host perl errors.
sub trap {
    die bless({ message => $_[0] }, 'Rt::Trap');
}
