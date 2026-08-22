# requires: rt/exit
sub wasi_proc_exit {
    my ($self, $code) = @_;
    die bless({ code => $code }, 'Rt::Exit');
}
