#!/usr/bin/env perl
# Interactive terminal frontend for the dewasm-generated NES library
# (nes_gen.pl, produced from agnes-based nes.wasm by build.sh).
# Like the
# DOOM Perl frontend (../../doom/perl) it renders into any ANSI truecolor terminal with half-block characters instead of opening a pixel window: a plain Perl interpreter is far too slow to hit the console's native 60Hz, but fast enough to render real frames into a terminal, which has orders of magnitude fewer cells to redraw than a window has pixels.
#
# Unlike DOOM's doom.wasm, nes.wasm has *zero* wasm imports (verified by examples/apps/scripts/nes.sh): there is no console/save-game/clock host surface to wire up, just `_initialize` plus the demo exports (allocRom/ initGame/setInput/tickGame/screenOffset/paletteOffset/frameWidth/ frameHeight).
# The guest hands over agnes's own frame representation -- one palette *index* per pixel plus the fixed 64-entry palette -- rather than a rendered image, which suits this renderer: a terminal cell samples one pixel out of several, so only the sampled indices are ever looked up.
# The controller is also level-triggered (one `setInput(bitmask)` call per tick, not DOOM's edge-triggered reportKeyDown/reportKeyUp pair), so the key-hold heuristic below just tracks which buttons are currently "down"
# and recomputes the bitmask every tick instead of invoking anything on press/release.
#
# Run with --smoke for a headless self-check (no tty needed): it loads the default ROM, inits the game, ticks it a fixed number of times, measures tick rate and render cost, and writes the final frame to screenshot.ppm.
# Core modules only; raw mode goes through stty (Term::ReadKey is not core).
use strict;
use warnings;
use Cwd ();
use FindBin ();
use IO::Select ();
use Time::HiRes qw(clock_gettime CLOCK_MONOTONIC);

require "$FindBin::Bin/nes_gen.pl";

use constant DEFAULT_ROM => "$FindBin::Bin/../../apps/cache/alter_ego.nes";

# Terminals deliver only key *presses*, so a press keeps a button "down" for this long after the last matching press/autorepeat before it's dropped from the bitmask passed to setInput.
# See ../../doom/perl/main.pl for the same heuristic under DOOM's edge-triggered import surface; here it just controls membership in the level-triggered bitmask instead of synthesizing a release call.
use constant KEY_HOLD_SECONDS => 0.4;

# Button bit assignment fixed by nes_demo.c's setInput contract.
use constant {
    BTN_A      => 1,
    BTN_B      => 2,
    BTN_SELECT => 4,
    BTN_START  => 8,
    BTN_UP     => 16,
    BTN_DOWN   => 32,
    BTN_LEFT   => 64,
    BTN_RIGHT  => 128,
};

my $HALF_BLOCK = "\xE2\x96\x80";    # U+2580 upper half block, as raw UTF-8

# Fixed status-line colors (white on black), independent of the game's own palette -- without an explicit color the status line inherits whatever fg/bg the last-drawn pixel cell left active, flickering with the game.
my $STATUS_SGR = "\e[48;2;0;0;0m\e[38;2;255;255;255m";

sub monotonic { return clock_gettime(CLOCK_MONOTONIC); }

sub load_rom {
    my ($path) = @_;
    open(my $fh, '<:raw', $path) or die "nes: cannot read ROM $path: $!\n";
    my $bytes = do { local $/; <$fh> };
    close($fh);
    return $bytes;
}

# Allocates the module, loads the ROM at $rom_path into guest memory via allocRom, and inits the emulator.
# Dies loudly if initGame rejects the ROM
# (bad iNES header, unsupported mapper, ...) rather than limping on.
sub init_nes {
    my ($rom_path) = @_;
    my $nes = Nes->new({});
    $nes->invoke('_initialize');
    my $rom = load_rom($rom_path);
    my $ptr = $nes->invoke('allocRom', length($rom));
    substr($nes->{memory}{data}, $ptr, length($rom)) = $rom;
    my $ok = $nes->invoke('initGame');
    die "nes: initGame failed on $rom_path (bad ROM?)\n" unless $ok == 1;
    return $nes;
}

# ---------------------------------------------------------------- Renderer
#
# Renders the frame into ANSI half-block terminal cells: each character cell shows two vertically-stacked source pixels via "▀" (foreground = top pixel, background = bottom pixel, both 24-bit truecolor SGR).
# Same diff strategy as the DOOM frontend: track the previous frame's cell contents and the cursor/SGR state, and only emit escape codes for cells that actually changed.
# Since a color is a function of its palette index, the whole SGR string per index is precomputed here and the diff compares indices.
package Renderer;

sub new {
    my ($class, $term_cols, $term_rows, $frame_w, $frame_h, $palette) = @_;
    my $status_rows = 1;
    my $avail_rows = $term_rows - $status_rows;
    $avail_rows = 1 if $avail_rows < 1;
    # Unlike DOOM's 640x400 (a 2x upscale of its native 320x200), the NES framebuffer is already at its native 256x240 resolution, so the natural cap is the frame width itself, not half of it.
    my $native_cols = $frame_w;
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
        fg_sgr      => [map { sprintf("\e[38;2;%d;%d;%dm", @{$_}) } @{$palette}],
        bg_sgr      => [map { sprintf("\e[48;2;%d;%d;%dm", @{$_}) } @{$palette}],
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
    my ($self, $screen, $frame_w, $frame_h, $status_text) = @_;
    my $buf = '';
    for my $cy (0 .. $self->{cell_rows} - 1) {
        my $top_row_base = int($cy * 2 * $frame_h / $self->{pixel_rows}) * $frame_w;
        my $bot_row_base = int(($cy * 2 + 1) * $frame_h / $self->{pixel_rows}) * $frame_w;
        my $prev_row = $self->{prev}[$cy];
        for my $cx (0 .. $self->{cell_cols} - 1) {
            my $src_x = int($cx * $frame_w / $self->{pixel_cols});
            # One byte per pixel, a palette index; the & 0x3f mask is load-bearing (see nes_demo.c).
            my $top = ord(substr($screen, $top_row_base + $src_x, 1)) & 0x3f;
            my $bot = ord(substr($screen, $bot_row_base + $src_x, 1)) & 0x3f;
            my $key = ($top << 6) | $bot;
            next if defined($prev_row->[$cx]) && $prev_row->[$cx] == $key;

            $prev_row->[$cx] = $key;
            unless (defined($self->{cursor_row}) && $self->{cursor_row} == $cy
                && defined($self->{cursor_col}) && $self->{cursor_col} == $cx) {
                $buf .= sprintf("\e[%d;%dH", $cy + 1, $cx + 1);
            }
            if (!defined($self->{last_fg}) || $self->{last_fg} != $top) {
                $self->{last_fg} = $top;
                $buf .= $self->{fg_sgr}[$top];
            }
            if (!defined($self->{last_bg}) || $self->{last_bg} != $bot) {
                $self->{last_bg} = $bot;
                $buf .= $self->{bg_sgr}[$bot];
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
# Terminals deliver only key *presses*, never releases.
# Unlike DOOM's reportKeyDown/reportKeyUp exports, nes.wasm's setInput takes the whole controller state as one bitmask per tick, so there is nothing to invoke on press or release: a press just marks its button bit "held" until
# KEY_HOLD_SECONDS pass with no matching repeat (terminal autorepeat just resends the same bytes, which pushes the deadline back), and the caller reads current_mask() once per tick to build the setInput argument.
package InputHandler;

my %ESCAPE_SEQUENCES = (
    "\e[A" => main::BTN_UP,
    "\e[B" => main::BTN_DOWN,
    "\e[C" => main::BTN_RIGHT,
    "\e[D" => main::BTN_LEFT,
);

sub new {
    my ($class) = @_;
    return bless({
        pending     => '',
        esc_seen_at => undef,
        held_until  => {},
        quit        => 0,
        select      => IO::Select->new(\*STDIN),
    }, $class);
}

sub quit { return $_[0]->{quit}; }

sub current_mask {
    my ($self) = @_;
    my $mask = 0;
    $mask |= $_ for keys %{ $self->{held_until} };
    return $mask;
}

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
            $self->key_down($ESCAPE_SEQUENCES{$seq}, $now);
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
        # Nothing in this frontend's key map uses Escape, so it's just dropped.
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
        return;
    }
    if ($byte == 0x0d) { $self->key_down(main::BTN_START,  $now); return; }    # Enter
    if ($byte == 0x20) { $self->key_down(main::BTN_SELECT, $now); return; }    # Space
    my $c = lc(chr($byte));
    if ($c eq 'x') { $self->key_down(main::BTN_A, $now); return; }
    if ($c eq 'z') { $self->key_down(main::BTN_B, $now); return; }
    return;
}

sub key_down {
    my ($self, $bit, $now) = @_;
    $self->{held_until}{$bit} = $now + main::KEY_HOLD_SECONDS;
    return;
}

sub expire_held_keys {
    my ($self, $now) = @_;
    for my $bit (grep { $now >= $self->{held_until}{$_} } keys %{ $self->{held_until} }) {
        delete $self->{held_until}{$bit};
    }
    return;
}

# ---------------------------------------------------------------- Drivers
package main;

sub write_ppm {
    my ($path, $w, $h, $screen, $palette) = @_;
    my @entries = map { pack('C3', @{$_}) } @{$palette};
    my $rgb = join('', map { $entries[$_ & 0x3f] } unpack('C*', $screen));
    open(my $fh, '>:raw', $path) or die "nes: cannot write $path: $!";
    print {$fh} "P6\n$w $h\n255\n", $rgb;
    close($fh);
    return Cwd::abs_path($path);
}

# The module's fixed 64-entry palette (R,G,B,A per entry; alpha is padding), read once -- unlike the index buffer, it never changes.
sub read_palette {
    my ($nes) = @_;
    my $bytes = substr($nes->{memory}{data}, $nes->invoke('paletteOffset'), 64 * 4);
    return [map { [unpack('C3', substr($bytes, $_ * 4, 3))] } 0 .. 63];
}

sub read_screen {
    my ($nes, $off, $w, $h) = @_;
    return substr($nes->{memory}{data}, $off, $w * $h);
}

sub run_smoke {
    my $rom_path = shift(@_) || DEFAULT_ROM;
    my $nes = init_nes($rom_path);
    my $w = $nes->invoke('frameWidth');
    my $h = $nes->invoke('frameHeight');
    my $screen_off = $nes->invoke('screenOffset');
    my $palette = read_palette($nes);

    # A synthetic terminal size, so this runs in CI/anywhere with no real tty.
    my $renderer = Renderer->new(160, 51, $w, $h, $palette);

    my $ticks = 40;
    my $render_seconds = 0.0;
    my $start = monotonic();
    for (1 .. $ticks) {
        $nes->invoke('setInput', 0);
        $nes->invoke('tickGame');
        my $r0 = monotonic();
        $renderer->render(read_screen($nes, $screen_off, $w, $h), $w, $h, 'smoke');
        $render_seconds += monotonic() - $r0;
    }
    my $elapsed = monotonic() - $start;
    printf(
        "smoke: ran %d ticks in %.3fs - %.2f ticks/sec bare, %.2f ticks/sec with terminal rendering (%.3fms/frame render)\n",
        $ticks, $elapsed, $ticks / ($elapsed - $render_seconds), $ticks / $elapsed, $render_seconds / $ticks * 1000
    );

    my $screen = read_screen($nes, $screen_off, $w, $h);
    my %distinct;
    $distinct{ join(',', @{ $palette->[$_ & 0x3f] }) } = 1 for unpack('C*', $screen);
    my $colors = scalar(keys %distinct);
    print "smoke: final frame is ${w}x${h} with $colors distinct colors\n";
    # agnes's NES palette is tiny (the Alter Ego credits screen this settles on measures 7 distinct colors); a degenerate (blank/solid) frame lands at 1.
    if ($colors <= 4) {
        print STDERR "smoke: FAIL: frame looks degenerate (too few distinct colors)\n";
        exit 1;
    }

    my $path = write_ppm('screenshot.ppm', $w, $h, $screen, $palette);
    print "smoke: wrote $path\n";
    return;
}

use constant ENTER_ALT_SCREEN => "\e[?1049h\e[?25l\e[2J\e[H";
# SGR reset first: the fixed status-line colors otherwise persist past leaving the alternate screen and tint the shell prompt underneath.
use constant EXIT_ALT_SCREEN  => "\e[0m\e[?25h\e[?1049l";

sub run_interactive {
    my $rom_path = shift(@_) || DEFAULT_ROM;

    unless (-t STDIN && -t STDOUT) {
        die "nes: interactive mode needs a real terminal on stdin/stdout (try `./run.sh --smoke` for a headless check)\n";
    }

    my $nes = init_nes($rom_path);
    my $w = $nes->invoke('frameWidth');
    my $h = $nes->invoke('frameHeight');
    # Both are stable for the emulator's lifetime, so they are read once.
    my $screen_off = $nes->invoke('screenOffset');
    my $palette = read_palette($nes);

    my ($rows, $cols) = split(' ', `stty size 2>/dev/null` || '');
    $rows ||= 24;
    $cols ||= 80;
    my $renderer = Renderer->new($cols, $rows, $w, $h, $palette);
    my $input = InputHandler->new();

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
    my $status_text = 'dewasm NES (Perl) | starting... | q/^C quit  arrows d-pad  x A  z B  enter start  space select';
    my $err = do {
        local $@;
        eval {
            while (1) {
                my $now = monotonic();
                $input->poll($now);
                last if $input->quit();

                $nes->invoke('setInput', $input->current_mask());
                $nes->invoke('tickGame');
                $status_ticks++;
                my $elapsed = $now - $status_window_start;
                if ($elapsed >= 0.5) {
                    $status_text = sprintf(
                        'dewasm NES (Perl) | %.2f ticks/sec | q/^C quit  arrows d-pad  x A  z B  enter start  space select',
                        $status_ticks / $elapsed
                    );
                    $status_ticks = 0;
                    $status_window_start = $now;
                }

                print $renderer->render(read_screen($nes, $screen_off, $w, $h), $w, $h, $status_text);
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
my @args = @ARGV;
my $smoke = 0;
if (@args && $args[0] eq '--smoke') {
    $smoke = 1;
    shift @args;
}
if ($smoke) {
    run_smoke(@args);
} else {
    run_interactive(@args);
}
