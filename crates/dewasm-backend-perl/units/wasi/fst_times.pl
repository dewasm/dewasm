# Validates the `fstflags` and computes the `(atime, mtime)` pair for `Time::HiRes::utime`.
# The pair is in NV seconds, and a field not being set comes from the current `stat` times.
# ATIM with ATIM_NOW (or MTIM with MTIM_NOW) is a contradiction and yields INVAL.
# Returns `(atime, mtime, err)`.
use Time::HiRes ();

sub fst_times {
    my ($self, $cur_atime, $cur_mtime, $atim, $mtim, $fst_flags) = @_;
    if ((($fst_flags & 0x1) && ($fst_flags & 0x2)) || (($fst_flags & 0x4) && ($fst_flags & 0x8))) {
        return (undef, undef, ERRNO_INVAL);
    }
    my $now = Time::HiRes::time();
    my $a = $fst_flags & 0x1 ? $atim / 1e9 : ($fst_flags & 0x2 ? $now : $cur_atime);
    my $m = $fst_flags & 0x4 ? $mtim / 1e9 : ($fst_flags & 0x8 ? $now : $cur_mtime);
    return ($a, $m, undef);
}
