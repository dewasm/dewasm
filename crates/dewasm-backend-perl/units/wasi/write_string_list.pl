# requires: memory/i32_store, memory/i32_store8, memory/init
sub write_string_list {
    my ($self, $strings, $list_ptr, $buf_ptr) = @_;
    for my $i (0 .. $#$strings) {
        my $s = $strings->[$i];
        $self->{memory}->i32_store($list_ptr + $i * 4, $buf_ptr);
        $self->{memory}->init($buf_ptr, $s, 0, length($s));
        $self->{memory}->i32_store8($buf_ptr + length($s), 0);
        $buf_ptr += length($s) + 1;
    }
    return ERRNO_SUCCESS;
}
