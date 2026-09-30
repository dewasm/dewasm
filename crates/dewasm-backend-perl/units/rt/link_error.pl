# A failed import resolution at instantiation time: a missing import, or one of the wrong kind.
# It is kept distinct from Trap and from plain Perl errors.
sub link_error {
    die bless({ message => $_[0] }, 'Rt::LinkError');
}
