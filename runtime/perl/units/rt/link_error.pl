# A failed import resolution at instantiation time (missing import, or one
# of the wrong kind), kept distinct from Trap and from plain perl errors.
sub link_error {
    die bless({ message => $_[0] }, 'Rt::LinkError');
}
