#!/usr/bin/env perl
# Interactive terminal frontend for the dewasm-generated DOOM library
# (doom_gen.pl, produced from jacobenget/doom.wasm by build.sh).
# Like the
# Ruby/Python/Bash frontends it renders into any ANSI truecolor terminal with half-block characters instead of opening a pixel window (see ../go,
# ../java): plain Perl runs DOOM at well under one tick/sec, hopeless for a
# GUI but fine for a terminal, which has orders of magnitude fewer cells to redraw than a window has pixels.
#
# Run with --smoke for a headless self-check (no tty needed): it inits the game, ticks it 10 times, measures tick rate and render cost, and writes the final frame to screenshot.ppm.
# Core modules only; raw mode goes through stty (Term::ReadKey is not core).
use strict;
use warnings;
use Cwd ();
use FindBin ();
use IO::Select ();
use Time::HiRes qw(clock_gettime CLOCK_MONOTONIC);

require "$FindBin::Bin/doom_gen.pl";

use constant SAVE_DIR => '.savegame';
# Terminals deliver only key *presses*, so a press is held "down" for this long after the last matching press/autorepeat before synthesizing the release.
# Wider than Ruby's 180ms window because this backend only manages
# ~0.7 ticks/sec, so polls (and therefore chances to notice an autorepeat)
# are over a second apart (same reasoning as the Python frontend).
use constant KEY_HOLD_SECONDS => 0.4;

my $HALF_BLOCK = "\xE2\x96\x80";    # U+2580 upper half block, as raw UTF-8

# Fixed status-line colors (white on black), independent of the game's own palette -- without an explicit color the status line inherits whatever fg/bg the last-drawn pixel cell left active, flickering with the game.
my $STATUS_SGR = "\e[48;2;0;0;0m\e[38;2;255;255;255m";

sub monotonic { return clock_gettime(CLOCK_MONOTONIC); }

sub save_game_path { return SAVE_DIR . "/doomsav$_[0].dsg"; }

# Wires the wasm module's ten host imports to Perl.
# `$doom_ref` exists because these closures have to be built before Doom->new returns the instance they read memory from; it's filled in immediately after construction and only dereferenced within calls the imports themselves receive later (never during Doom->new itself).
sub build_imports {
    my ($doom_ref, $frame, $suppress_info) = @_;
    my $mem_string = sub { return substr($$doom_ref->{memory}{data}, $_[0], $_[1]); };
    my $emit = sub {
        my ($fh, $s) = @_;
        $s .= "\n" unless $s =~ /\n\z/;
        print {$fh} $s;
    };
    return {
        'console' => {
            'onErrorMessage' => sub { $emit->(\*STDERR, $mem_string->(@_)); },
            'onInfoMessage'  => sub {
                # Info messages would corrupt the ANSI frame while the alternate screen is active, so they're dropped in interactive mode; --smoke has no alternate screen and prints them normally.
                return if $suppress_info;
                $emit->(\*STDOUT, $mem_string->(@_));
            },
        },
        'gameSaving' => {
            'sizeOfSaveGame' => sub { return (-s save_game_path($_[0])) || 0; },
            'readSaveGame'   => sub {
                my ($id, $dst_off) = @_;
                open(my $fh, '<:raw', save_game_path($id)) or return 0;
                my $bytes = do { local $/; <$fh> };
                close($fh);
                substr($$doom_ref->{memory}{data}, $dst_off, length($bytes)) = $bytes;
                return length($bytes);
            },
            'writeSaveGame' => sub {
                my ($id, $src_off, $len) = @_;
                mkdir(SAVE_DIR) unless -d SAVE_DIR;
                open(my $fh, '>:raw', save_game_path($id)) or return 0;
                print {$fh} $mem_string->($src_off, $len);
                close($fh);
                return $len;
            },
        },
        'runtimeControl' => {
            # Backs DOOM's internal 35Hz pacing, so it has to be a real monotonic clock (not a fake stepped one) or the game's notion of elapsed time would drift from how often we call tickGame.
            'timeInMilliseconds' => sub { return int(monotonic() * 1000); },
        },
        'ui' => {
            # Captured as one immediate bulk substr copy, not scanned pixel-by-pixel: a per-pixel memory call here would be ~256k calls/frame and would dominate the whole tick budget.
            'drawFrame' => sub {
                my ($buf_off) = @_;
                my $len = $frame->{w} * $frame->{h} * 4;
                $frame->{pixels} = $mem_string->($buf_off, $len);
            },
        },
        'loading' => {
            'onGameInit' => sub { ($frame->{w}, $frame->{h}) = @_; },
            # Leaving both output slots untouched (they arrive pre-zeroed)
            # selects the wasm-embedded shareware WAD; supplying external
            # WADs is out of scope for this frontend.
            'wadSizes' => sub { },
            'readWads' => sub { },
        },
    };
}

sub key_map {
    my ($doom) = @_;
    return {
        up        => $doom->global_get('KEY_UPARROW'),
        down      => $doom->global_get('KEY_DOWNARROW'),
        left      => $doom->global_get('KEY_LEFTARROW'),
        right     => $doom->global_get('KEY_RIGHTARROW'),
        strafe_l  => $doom->global_get('KEY_STRAFE_L'),
        strafe_r  => $doom->global_get('KEY_STRAFE_R'),
        fire      => $doom->global_get('KEY_FIRE'),
        use       => $doom->global_get('KEY_USE'),
        tab       => $doom->global_get('KEY_TAB'),
        escape    => $doom->global_get('KEY_ESCAPE'),
        enter     => $doom->global_get('KEY_ENTER'),
        backspace => $doom->global_get('KEY_BACKSPACE'),
        # KEY_SHIFT/KEY_ALT exist as exports too, but Shift (run) and Alt have no terminal-deliverable equivalent, so they're never looked up here.
    };
}

# ---------------------------------------------------------------- Renderer
#
# Renders the framebuffer into ANSI half-block terminal cells: each character cell shows two vertically-stacked source pixels via "▀"
# (foreground = top pixel, background = bottom pixel, both 24-bit truecolor
# SGR).
# This is the performance-sensitive part of this frontend in the other frontends' languages (much less so in Perl, where a single tick already costs over a second), but the same diff strategy is kept: track the previous frame's cell contents and the cursor/SGR state, and only emit escape codes for cells that actually changed (DOOM's software renderer is paletted, so most cells repeat exactly from one frame to the next).
package Renderer;

sub new {
    my ($class, $term_cols, $term_rows, $frame_w, $frame_h) = @_;
    my $status_rows = 1;
    my $avail_rows = $term_rows - $status_rows;
    $avail_rows = 1 if $avail_rows < 1;
    # 320x200 (DOOM's native, pre-2x-upscale resolution) is the natural cap:
    # sampling beyond one source pixel per 2 is not showing any more detail.
    my $native_cols = int($frame_w / 2);
    my $pixel_cols = $term_cols < $native_cols ? $term_cols : $native_cols;
    my $pixel_rows = int($pixel_cols * $frame_h / $frame_w + 0.5);
    my $cell_rows = int($pixel_rows / 2);
    if ($cell_rows > $avail_rows) {
        $cell_rows = $avail_rows;
        $pixel_rows = $cell_rows * 2;
        my $fit = int($pixel_rows * $frame_w / $frame_h + 0.5);
        $pixel_cols = (sort { $a <=> $b } ($fit, $term_cols, $native_cols))[0];
    }
    return bless({
        pixel_cols  => $pixel_cols,
        pixel_rows  => $pixel_rows,
        cell_cols   => $pixel_cols,
        cell_rows   => $cell_rows,
        prev        => [map { [(undef) x $pixel_cols] } 1 .. $cell_rows],
        cursor_row  => undef,
        cursor_col  => undef,
        last_fg     => undef,
        last_bg     => undef,
        last_status => undef,
    }, $class);
}

sub cell_cols { return $_[0]->{cell_cols}; }
sub cell_rows { return $_[0]->{cell_rows}; }

# Builds one frame's worth of escape sequences/characters as a single string; the caller is responsible for writing it (or, for --smoke, just timing how long this took and discarding it).
sub render {
    my ($self, $pixels, $frame_w, $frame_h, $status_text) = @_;
    my $buf = '';
    for my $cy (0 .. $self->{cell_rows} - 1) {
        my $top_row_base = int($cy * 2 * $frame_h / $self->{pixel_rows}) * $frame_w;
        my $bot_row_base = int(($cy * 2 + 1) * $frame_h / $self->{pixel_rows}) * $frame_w;
        my $prev_row = $self->{prev}[$cy];
        for my $cx (0 .. $self->{cell_cols} - 1) {
            my $src_x = int($cx * $frame_w / $self->{pixel_cols});
            my $to = ($top_row_base + $src_x) * 4;
            my $bo = ($bot_row_base + $src_x) * 4;
            # Memory byte order is B,G,R,A (see loading.onGameInit callers).
            my $top_r = ord(substr($pixels, $to + 2, 1));
            my $top_g = ord(substr($pixels, $to + 1, 1));
            my $top_b = ord(substr($pixels, $to, 1));
            my $bot_r = ord(substr($pixels, $bo + 2, 1));
            my $bot_g = ord(substr($pixels, $bo + 1, 1));
            my $bot_b = ord(substr($pixels, $bo, 1));
            my $key = ($top_r << 40) | ($top_g << 32) | ($top_b << 24) | ($bot_r << 16) | ($bot_g << 8) | $bot_b;
            next if defined($prev_row->[$cx]) && $prev_row->[$cx] == $key;

            $prev_row->[$cx] = $key;
            unless (defined($self->{cursor_row}) && $self->{cursor_row} == $cy
                && defined($self->{cursor_col}) && $self->{cursor_col} == $cx) {
                $buf .= sprintf("\e[%d;%dH", $cy + 1, $cx + 1);
            }
            my $fg = $key >> 24;
            if (!defined($self->{last_fg}) || $self->{last_fg} != $fg) {
                $self->{last_fg} = $fg;
                $buf .= "\e[38;2;$top_r;$top_g;${top_b}m";
            }
            my $bg = $key & 0xffffff;
            if (!defined($self->{last_bg}) || $self->{last_bg} != $bg) {
                $self->{last_bg} = $bg;
                $buf .= "\e[48;2;$bot_r;$bot_g;${bot_b}m";
            }
            $buf .= $HALF_BLOCK;
            $self->{cursor_row} = $cy;
            $self->{cursor_col} = $cx + 1;
        }
    }
    if (!defined($self->{last_status}) || $status_text ne $self->{last_status}) {
        # Reset SGR first: otherwise the status line inherits whichever fg/bg the last-drawn pixel cell left active, making its background flicker with the game's own colors instead of staying the terminal default.
        $buf .= sprintf("\e[%d;1H\e[0m%s\e[K%s", $self->{cell_rows} + 1, $STATUS_SGR, $status_text);
        $self->{last_status} = $status_text;
        # Force the next painted cell to reposition: the cursor is now on the status line.
        $self->{cursor_row} = -1;
        # The reset above invalidated the SGR cache; force the next cell to re-emit its color.
        $self->{last_fg} = undef;
        $self->{last_bg} = undef;
    }
    return $buf;
}

# ------------------------------------------------------------------ Input
#
# Terminals deliver only key *presses*, never releases, so a press synthesizes both an immediate reportKeyDown and a reportKeyUp once
# KEY_HOLD_SECONDS pass with no matching repeat (terminal autorepeat just resends the same bytes, which pushes the deadline back via key_down).
package InputHandler;

my %ESCAPE_SEQUENCES = (
    "\e[A" => 'up',
    "\e[B" => 'down',
    "\e[C" => 'right',
    "\e[D" => 'left',
);

sub new {
    my ($class, $doom, $keys) = @_;
    return bless({
        doom        => $doom,
        keys        => $keys,
        pending     => '',
        esc_seen_at => undef,
        held_until  => {},
        quit        => 0,
        select      => IO::Select->new(\*STDIN),
    }, $class);
}

sub quit { return $_[0]->{quit}; }

sub poll {
    my ($self, $now) = @_;
    $self->read_available();
    $self->process_pending($now);
    $self->expire_held_keys($now);
    return;
}

sub read_available {
    my ($self) = @_;
    while ($self->{select}->can_read(0)) {
        my $n = sysread(STDIN, my $chunk, 64);
        last unless $n;
        $self->{pending} .= $chunk;
    }
    return;
}

sub process_pending {
    my ($self, $now) = @_;
    while (length($self->{pending})) {
        if (ord(substr($self->{pending}, 0, 1)) == 0x1b) {
            last unless $self->process_escape($now);
            next;
        }
        my $byte = ord(substr($self->{pending}, 0, 1, ''));
        $self->handle_byte($byte, $now);
    }
    return;
}

# Returns true if it consumed (or decided to drop) something from pending, false if it needs more bytes and the caller should stop polling for this tick.
sub process_escape {
    my ($self, $now) = @_;
    my $pending = $self->{pending};
    if (length($pending) >= 3) {
        my ($seq) = grep { index($pending, $_) == 0 } keys %ESCAPE_SEQUENCES;
        if (defined $seq) {
            $self->key_down($self->{keys}{ $ESCAPE_SEQUENCES{$seq} }, $now);
            substr($self->{pending}, 0, length($seq)) = '';
        } else {
            # Not one of our known arrow sequences (e.g. an F-key or
            # Home/End CSI sequence): drop just the ESC byte and reprocess the rest as ordinary bytes rather than losing them.
            substr($self->{pending}, 0, 1) = '';
        }
        $self->{esc_seen_at} = undef;
        return 1;
    }

    if (length($pending) == 2 && ord(substr($pending, 1, 1)) != 0x5b) {    # second byte isn't '['
        substr($self->{pending}, 0, 1) = '';
        $self->{esc_seen_at} = undef;
        return 1;
    }

    if ($pending eq "\e" && defined($self->{esc_seen_at})) {
        # Still a bare ESC on a second poll with no growth: a real Escape key press, not the start of a sequence still in flight.
        $self->key_down($self->{keys}{escape}, $now);
        $self->{pending} = '';
        $self->{esc_seen_at} = undef;
        return 1;
    }

    # "\e" (seen for the first time) or "\e[" (a valid prefix so far):
    # wait for the rest to arrive on a later poll.
    $self->{esc_seen_at} = $now if $pending eq "\e" && !defined($self->{esc_seen_at});
    return 0;
}

sub handle_byte {
    my ($self, $byte, $now) = @_;
    if ($byte == 0x03 || $byte == 0x71) {    # Ctrl-C, 'q'
        $self->{quit} = 1;
    } elsif ($byte == 0x0d) { $self->key_down($self->{keys}{enter}, $now); }
    elsif ($byte == 0x09) { $self->key_down($self->{keys}{tab}, $now); }
    elsif ($byte == 0x7f) { $self->key_down($self->{keys}{backspace}, $now); }
    elsif ($byte == 0x2c) { $self->key_down($self->{keys}{strafe_l}, $now); }
    elsif ($byte == 0x2e) { $self->key_down($self->{keys}{strafe_r}, $now); }
    elsif ($byte == 0x66) { $self->key_down($self->{keys}{fire}, $now); }
    elsif ($byte == 0x20) { $self->key_down($self->{keys}{use}, $now); }
    else {
        my $c = lc(chr($byte));
        $self->key_down(ord($c), $now) if $c =~ /[a-z0-9]/;
    }
    return;
}

sub key_down {
    my ($self, $code, $now) = @_;
    $self->{doom}->invoke('reportKeyDown', $code);
    $self->{held_until}{$code} = $now + main::KEY_HOLD_SECONDS;
    return;
}

sub expire_held_keys {
    my ($self, $now) = @_;
    for my $code (grep { $now >= $self->{held_until}{$_} } keys %{ $self->{held_until} }) {
        $self->{doom}->invoke('reportKeyUp', $code);
        delete $self->{held_until}{$code};
    }
    return;
}

# ---------------------------------------------------------------- Drivers
package main;

sub write_ppm {
    my ($path, $w, $h, $pixels) = @_;
    # Memory byte order is B,G,R,A; PPM wants R,G,B (alpha is padding).
    (my $rgb = $pixels) =~ s/(.)(.)(.)./$3$2$1/gs;
    open(my $fh, '>:raw', $path) or die "doom: cannot write $path: $!";
    print {$fh} "P6\n$w $h\n255\n", $rgb;
    close($fh);
    return Cwd::abs_path($path);
}

sub run_smoke {
    my $frame = { w => 0, h => 0, pixels => undef };
    my $doom;
    $doom = Doom->new(build_imports(\$doom, $frame, 0));

    $doom->invoke('initGame');
    if (!$frame->{w}) {
        print STDERR "smoke: FAIL: initGame never triggered loading.onGameInit\n";
        exit 1;
    }

    # A synthetic terminal size, so this runs in CI/anywhere with no real tty.
    my $renderer = Renderer->new(160, 51, $frame->{w}, $frame->{h});

    my $ticks = 10;
    my $render_seconds = 0.0;
    my $start = monotonic();
    for (1 .. $ticks) {
        $doom->invoke('tickGame');
        my $r0 = monotonic();
        $renderer->render($frame->{pixels}, $frame->{w}, $frame->{h}, 'smoke');
        $render_seconds += monotonic() - $r0;
    }
    my $elapsed = monotonic() - $start;
    printf(
        "smoke: ran %d ticks in %.3fs - %.2f ticks/sec bare, %.2f ticks/sec with terminal rendering (%.3fms/frame render)\n",
        $ticks, $elapsed, $ticks / ($elapsed - $render_seconds), $ticks / $elapsed, $render_seconds / $ticks * 1000
    );

    my $pixels = $frame->{pixels};
    if (!defined $pixels) {
        print STDERR "smoke: FAIL: no frame was ever captured\n";
        exit 1;
    }

    my ($w, $h) = ($frame->{w}, $frame->{h});
    my %distinct;
    for (my $o = 0; $o < length($pixels); $o += 4) {
        $distinct{ substr($pixels, $o, 3) } = 1;
    }
    my $colors = scalar(keys %distinct);
    print "smoke: final frame is ${w}x${h} with $colors distinct colors\n";
    # DOOM's software renderer is paletted (classic VGA Mode 13h: at most
    # 256 colors), so a healthy frame tops out in the low hundreds, not the thousands a truecolor renderer would produce.
    # A degenerate frame
    # (blank/solid) instead lands in the single digits.
    if ($colors <= 50) {
        print STDERR "smoke: FAIL: frame looks degenerate (too few distinct colors)\n";
        exit 1;
    }

    my $path = write_ppm('screenshot.ppm', $w, $h, $pixels);
    print "smoke: wrote $path\n";
    return;
}

use constant ENTER_ALT_SCREEN => "\e[?1049h\e[?25l\e[2J\e[H";
# SGR reset first: the fixed status-line colors otherwise persist past leaving the alternate screen and tint the shell prompt underneath.
use constant EXIT_ALT_SCREEN  => "\e[0m\e[?25h\e[?1049l";

sub run_interactive {
    unless (-t STDIN && -t STDOUT) {
        die "doom: interactive mode needs a real terminal on stdin/stdout (try `./run.sh --smoke` for a headless check)\n";
    }

    my $frame = { w => 0, h => 0, pixels => undef };
    my $doom;
    $doom = Doom->new(build_imports(\$doom, $frame, 1));
    $doom->invoke('initGame');

    my ($rows, $cols) = split(' ', `stty size 2>/dev/null` || '');
    $rows ||= 24;
    $cols ||= 80;
    my $renderer = Renderer->new($cols, $rows, $frame->{w}, $frame->{h});
    my $input = InputHandler->new($doom, key_map($doom));

    my $orig_stty = `stty -g`;
    chomp $orig_stty;
    my $restored = 0;
    my $restore = sub {
        return if $restored;
        $restored = 1;
        system('stty', $orig_stty) if $orig_stty;
        print EXIT_ALT_SCREEN;
    };
    # Ctrl-C is handled explicitly as a byte in InputHandler because raw mode disables the terminal's own SIGINT generation; these handlers are only a backstop for termination from outside (e.g. `kill`).
    local $SIG{INT}  = sub { $restore->(); exit 0; };
    local $SIG{TERM} = sub { $restore->(); exit 0; };

    system('stty', 'raw', '-echo');
    print ENTER_ALT_SCREEN;

    my $status_window_start = monotonic();
    my $status_ticks = 0;
    my $status_text = 'dewasm DOOM (Perl) | starting... | q/^C quit  f fire  space use  arrows move  ,/. strafe  tab automap';
    my $err = do {
        local $@;
        eval {
            while (1) {
                my $now = monotonic();
                $input->poll($now);
                last if $input->quit();

                $doom->invoke('tickGame');
                $status_ticks++;
                my $elapsed = $now - $status_window_start;
                if ($elapsed >= 0.5) {
                    $status_text = sprintf(
                        'dewasm DOOM (Perl) | %.2f ticks/sec | q/^C quit  f fire  space use  arrows move  ,/. strafe  tab automap',
                        $status_ticks / $elapsed
                    );
                    $status_ticks = 0;
                    $status_window_start = $now;
                }

                print $renderer->render($frame->{pixels}, $frame->{w}, $frame->{h}, $status_text);
            }
        };
        $@;
    };
    $restore->();
    die $err if $err;
    return;
}

binmode(STDOUT);
binmode(STDERR);
STDOUT->autoflush(1);
if (grep { $_ eq '--smoke' } @ARGV) {
    run_smoke();
} else {
    run_interactive();
}
