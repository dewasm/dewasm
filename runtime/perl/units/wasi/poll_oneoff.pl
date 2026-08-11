# requires: memory/fill, memory/i32_load, memory/i32_load8_u, memory/i32_load16_u, memory/i64_load, memory/i32_store, memory/i32_store8, memory/i32_store16, memory/i64_store
# poll_oneoff waits until at least one subscription is ready, then writes
# one event per ready subscription (WASI p1 layout: 48-byte subscriptions
# in, 32-byte events out). Only fd_read on stdin actually blocks (via
# 4-arg select); regular files, stdout/stderr, and every fd_write are
# treated as immediately ready, and unknown fds report EBADF. Clock
# subscriptions set the wait deadline; if it elapses with no fd ready, the
# due clock subs fire. Motivated by event-loop guests such as the QuickJS
# REPL, which blocks here on stdin between prompts.
use Time::HiRes ();

sub wasi_poll_oneoff {
    my ($self, $in_ptr, $out_ptr, $nsubs, $nevents_ptr) = @_;
    return ERRNO_INVAL if $nsubs == 0;
    my @ready;    # [userdata, error, type, nbytes, flags] resolvable without waiting
    my @waiters;  # [userdata, type, fh] fd_read on stdin: needs a host wait
    my @clocks;   # [userdata, rel_ns]
    for my $i (0 .. $nsubs - 1) {
        my $base = $in_ptr + $i * 48;
        my $userdata = $self->{memory}->i64_load($base);
        my $tag = $self->{memory}->i32_load8_u($base + 8);
        if ($tag == 0) {  # clock
            my $clock_id = $self->{memory}->i32_load($base + 16);
            my $timeout = $self->{memory}->i64_load($base + 24);
            my $flags = $self->{memory}->i32_load16_u($base + 40);
            my $now = $clock_id == 0
                ? int(Time::HiRes::time() * 1e9)
                : int(Time::HiRes::clock_gettime(Time::HiRes::CLOCK_MONOTONIC()) * 1e9);
            my $rel = ($flags & 1) ? ($timeout > $now ? $timeout - $now : 0) : $timeout;
            push @clocks, [$userdata, $rel];
        } elsif ($tag == 1 || $tag == 2) {  # fd_read / fd_write
            my $fd = $self->{memory}->i32_load($base + 16);
            my $e = $self->{fds}{$fd};
            if (!defined($e) || $e->{dir}) {
                push @ready, [$userdata, ERRNO_BADF, $tag, 0, 0];
            } elsif ($tag == 1 && defined($e->{std}) && $e->{std} == 0) {
                push @waiters, [$userdata, $tag, $e->{fh}];
            } else {
                my $nbytes = 1;
                if ($tag == 1 && !defined($e->{std})) {
                    my $size = (stat($e->{fh}))[7];
                    my $pos = sysseek($e->{fh}, 0, 1);
                    $nbytes = defined($size) && defined($pos) && $size > $pos ? $size - $pos : 0;
                }
                push @ready, [$userdata, 0, $tag, $nbytes, 0];
            }
        } else {
            return ERRNO_INVAL;
        }
    }

    my @events = @ready;
    if (!@events) {
        if (@waiters) {
            my $timeout_s;
            if (@clocks) {
                my $min = $clocks[0][1];
                $min = $_->[1] < $min ? $_->[1] : $min for @clocks;
                $timeout_s = $min / 1e9;
            }
            my $rin = '';
            vec($rin, fileno($_->[2]), 1) = 1 for @waiters;
            my $rout = $rin;
            select($rout, undef, undef, $timeout_s);
            for my $w (@waiters) {
                push @events, [$w->[0], 0, $w->[1], 1, 0] if vec($rout, fileno($w->[2]), 1);
            }
        } elsif (@clocks) {
            my $min = $clocks[0][1];
            $min = $_->[1] < $min ? $_->[1] : $min for @clocks;
            Time::HiRes::sleep($min / 1e9) if $min > 0;
        }
        if (!@events && @clocks) {
            my $due = $clocks[0][1];
            $due = $_->[1] < $due ? $_->[1] : $due for @clocks;
            for my $c (@clocks) {
                push @events, [$c->[0], 0, 0, 0, 0] if $c->[1] <= $due;
            }
        }
    }

    for my $i (0 .. $#events) {
        my ($userdata, $error, $type, $nbytes, $flags) = @{$events[$i]};
        my $ev = $out_ptr + $i * 32;
        $self->{memory}->fill($ev, 0, 32);
        $self->{memory}->i64_store($ev, $userdata);
        $self->{memory}->i32_store16($ev + 8, $error);
        $self->{memory}->i32_store8($ev + 10, $type);
        $self->{memory}->i64_store($ev + 16, $nbytes);
        $self->{memory}->i32_store16($ev + 24, $flags);
    }
    $self->{memory}->i32_store($nevents_ptr, scalar @events);
    return ERRNO_SUCCESS;
}
