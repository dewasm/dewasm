# The proc_exit carrier, caught by the standalone entrypoint.
sub exit_program {
    die bless({ code => $_[0] }, 'Rt::Exit');
}
