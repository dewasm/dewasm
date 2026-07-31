sub slice {
    my ($self, $offset, $len) = @_;
    return @{$self->{slots}}[$offset .. $offset + $len - 1];
}
