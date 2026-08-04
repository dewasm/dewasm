//! Perl end-to-end suites (ADR-27): the shared case consts (`dewasm-test-helper`) wired up for the Perl backend. Per the ADR-27 revision this file holds ONLY the [`BackendUnderTest`] impl, named glue string constants, and per-case macro invocations. Perl covers full WASI preview 1 incl. the filesystem (ADR-55/issue #69), so it wires every WASI kind, the slow-tier `apps`/`fs_apps`/`capi` suites, and both multi-module cases (the Embedded runtime is prefix-namespaced per package, so two artifacts coexist).

use std::path::PathBuf;

use dewasm_backend::{Backend, Mode, RuntimeLinkage};
use dewasm_backend_perl::{find_perl, PerlBackend};
use dewasm_test_helper::BackendUnderTest;

pub struct Perl;

impl BackendUnderTest for Perl {
    fn name(&self) -> &'static str {
        "perl"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &PerlBackend
    }

    fn interpreter(&self) -> PathBuf {
        find_perl()
            .expect("perl >= 5.26 with 64-bit IVs/NVs not found on PATH — see docs/testing.md")
    }

    /// Compose several `.wat` modules. `shared_runtime` emits each against one top-tier `Rt` (Alias linkage) plus a single bundled runtime, so an imported table crosses modules; otherwise it concatenates independent Embedded conversions (each carrying its own `<Package>::Rt`).
    fn compose_modules(&self, modules: &[(&str, &str)], shared_runtime: bool) -> String {
        if shared_runtime {
            let mut units = std::collections::BTreeSet::new();
            let mut pkgs = Vec::new();
            for (wat, name) in modules {
                let bytes = wat::parse_file(dewasm_test_helper::examples_dir().join(wat))
                    .expect("parse wat");
                let module = dewasm_core::build_module(&bytes).expect("build IR");
                let (src, u) = dewasm_backend_perl::generate_package_with_units(
                    &module,
                    name,
                    &RuntimeLinkage::Alias("Rt".to_string()),
                    false,
                )
                .expect("generate");
                units.extend(u);
                pkgs.push(src);
            }
            format!(
                "{}\n{}",
                dewasm_backend_perl::shared_runtime(&units).expect("bundle runtime"),
                pkgs.join("\n")
            )
        } else {
            modules
                .iter()
                .map(|(wat, name)| {
                    dewasm_test_helper::convert(
                        &PerlBackend,
                        &dewasm_test_helper::examples_dir().join(wat),
                        Mode::Library,
                        name,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

// --------------------------------------------------------------------- Library-case glue.

/// `add.wat`: call the exported functions and print each result.
const PERL_ADD_GLUE: &str = r#"my $inst = Add->new({});
print $inst->invoke('add', 2, 3), "\n";
print $inst->invoke('add', 0xffffffff, 1), "\n";
print $inst->invoke('fib', 10), "\n";
"#;

/// The ADR-7 override/fallback glue: fd_write intercepted, random_get falls back to the bundled WASI. Prints the actual bytes written.
const PERL_OVERRIDE_GLUE: &str = r#"my $inst;
my $captured = '';
my $fd_write = sub {
    my ($fd, $iovs, $iovs_len, $out_ptr) = @_;
    my $mem = $inst->{memory};
    my $ptr = $mem->i32_load($iovs);
    my $len = $mem->i32_load($iovs + 4);
    $captured .= $mem->read_string($ptr, $len);
    $mem->i32_store($out_ptr, $len);
    return 0;
};
$inst = Prog->new({ 'wasi_snapshot_preview1' => { 'fd_write' => $fd_write } });
$inst->invoke('_start');  # random_get falls back to the bundled WASI
print $captured;
"#;

/// The `custom_wasi_provider` glue: a provider *object* (`wasm_import`/`attach`, the Perl analog of Python's provider) covers every import, so the bundled WASI (`_wasi`) is never lazily constructed.
const PERL_CUSTOM_PROVIDER_GLUE: &str = r#"package MyWasi {
    sub new { return bless({ out => '' }, shift); }
    sub wasm_import {
        my ($self, $name) = @_;
        return sub { return $self->fd_write(@_); } if $name eq 'fd_write';
        return sub { return 0; } if $name eq 'random_get';
        return undef;
    }
    sub attach {
        my ($self, $instance) = @_;
        $self->{memory} = $instance->{memory};
    }
    sub fd_write {
        my ($self, $fd, $iovs, $iovs_len, $out_ptr) = @_;
        my $ptr = $self->{memory}->i32_load($iovs);
        my $len = $self->{memory}->i32_load($iovs + 4);
        $self->{out} .= $self->{memory}->read_string($ptr, $len);
        $self->{memory}->i32_store($out_ptr, $len);
        return 0;
    }
}
my $wasi = MyWasi->new();
my $inst = Prog->new({ 'wasi_snapshot_preview1' => $wasi });
$inst->invoke('_start');
print $wasi->{out};
print 'bundled wasi constructed: ', (defined $inst->{_wasi} ? 'true' : 'false'), "\n";
"#;

/// The `partial_override_falls_back_to_bundled_wasi` glue: fd_write intercepted, random_get falls back — so the bundled WASI *was* lazily constructed.
const PERL_PARTIAL_OVERRIDE_GLUE: &str = r#"my $inst;
my $captured = '';
my $fd_write = sub {
    my ($fd, $iovs, $iovs_len, $out_ptr) = @_;
    my $mem = $inst->{memory};
    my $ptr = $mem->i32_load($iovs);
    my $len = $mem->i32_load($iovs + 4);
    $captured .= $mem->read_string($ptr, $len);
    $mem->i32_store($out_ptr, $len);
    return 0;
};
$inst = Prog->new({ 'wasi_snapshot_preview1' => { 'fd_write' => $fd_write } });
$inst->invoke('_start');  # random_get falls back to the bundled WASI
print $captured;
print 'bundled wasi constructed: ', (defined $inst->{_wasi} ? 'true' : 'false'), "\n";
"#;

/// The `wasi_stdio_capture` glue: redirect STDOUT's file descriptor to an anonymous (unlinked) temp file — perl's native embedder-controlled sink that still supports the runtime's raw syswrite — run, restore, then print the captured bytes to the real stdout.
const PERL_STDIO_CAPTURE_GLUE: &str = r#"open(my $cap, '+>', undef) or die "capture: $!";
open(my $saved, '>&', \*STDOUT) or die "dup: $!";
open(STDOUT, '>&', $cap) or die "redirect: $!";
eval {
    my $inst = Prog->new({});
    $inst->invoke('_start');
};
my $err = $@;
open(STDOUT, '>&', $saved) or die "restore: $!";
die $err if $err && !(ref($err) && $err->isa('Prog::Rt::Exit'));
sysseek($cap, 0, 0);
my $data = '';
1 while sysread($cap, $data, 65536, length($data));
binmode(STDOUT);
print $data;
"#;

// --------------------------------------------------------------------- WASI filesystem glue.

/// The shared filesystem template: preopen the scratch dir (`{host}`) at guest `{guest}` (always `/`), run `_start`, and surface a `proc_exit` code as a trailing decimal line.
const PERL_FS_GLUE: &str = r#"my $inst = Prog->new({}, preopens => { '{guest}' => '{host}' });
eval { $inst->invoke('_start'); };
if (my $e = $@) {
    die $e unless ref($e) && $e->isa('Prog::Rt::Exit');
    print $e->{code}, "\n";
}
"#;

/// The root-preopen containment probe: call the WASI resolver directly with a `"/" => "/"` preopen (no guest run) and normalize the outcome to `contained`.
const PERL_CONTAINMENT_GLUE: &str = r#"my $wasi = Prog::Rt::WASI->new(preopens => { '/' => '/' });
my ($path, $err) = $wasi->resolve_path(3, 'etc');
print defined $err ? "rejected\n" : "contained\n";
"#;

// --------------------------------------------------------------------- Filesystem app glue: package/argv/env/preopen-guest-paths are literals; only the host scratch/cache dirs come through {scratch}/{cache}.

const PERL_QJS_FILE_IO_GLUE: &str = r#"my $inst = Qjs->new({}, args => ['qjs', '/work/qjs_file_io.js'], env => {}, preopens => { '/work' => '{scratch}' });
eval { $inst->invoke('_start'); };
die $@ if $@ && !(ref($@) && $@->isa('Qjs::Rt::Exit'));
"#;

const PERL_SQLITE3_SHELL_GLUE: &str = r#"my $inst = Sqlite3Shell->new({}, args => ['sqlite3'], env => {}, preopens => { '/db' => '{scratch}' });
eval { $inst->invoke('_start'); };
die $@ if $@ && !(ref($@) && $@->isa('Sqlite3Shell::Rt::Exit'));
"#;

const PERL_RG_SEARCH_GLUE: &str = r#"my $inst = Rg->new({}, args => ['rg', '--sort', 'path', 'needle', '/work'], env => {}, preopens => { '/work' => '{scratch}' });
eval { $inst->invoke('_start'); };
die $@ if $@ && !(ref($@) && $@->isa('Rg::Rt::Exit'));
"#;

const PERL_CPYTHON_GLUE: &str = r#"my $inst = Cpython->new({}, args => ['python', '-c', "print('hello from cpython', 6 * 7)"], env => { 'PYTHONHOME' => '/', 'PYTHONPATH' => '/lib/python3.14' }, preopens => { '/lib' => '{cache}/cpython-lib/lib' });
eval { $inst->invoke('_start'); };
die $@ if $@ && !(ref($@) && $@->isa('Cpython::Rt::Exit'));
"#;

const PERL_CRUBY_GLUE: &str = r#"my $inst = Cruby->new({}, args => ['ruby', '-e', 'puts "hello from cruby #{6*7}"'], env => {}, preopens => { '/usr' => '{cache}/ruby-lib/usr' });
eval { $inst->invoke('_start'); };
die $@ if $@ && !(ref($@) && $@->isa('Cruby::Rt::Exit'));
"#;

// --------------------------------------------------------------------- C-API drive glue (sqlite3): malloc/pointer plumbing via the memory object. Only the file-backed case uses {scratch}.

const PERL_LIBSQLITE3_MEM: &str = r#"
my $db = Libsqlite3->new({});
$db->invoke('_initialize');
my $mem = $db->{memory};

my $read_cstr = sub {
    my ($ptr) = @_;
    return undef if $ptr == 0;
    my $end = index($mem->{data}, "\0", $ptr);
    return $mem->read_string($ptr, $end - $ptr);
};

my $cstr = sub {
    my ($s) = @_;
    my $b = $s . "\0";
    my $p = $db->invoke('sqlite3_malloc', length($b));
    $mem->init($p, $b, 0, length($b));
    return $p;
};

print 'version: ', $read_cstr->($db->invoke('sqlite3_libversion')), "\n";

my $pp_db = $db->invoke('sqlite3_malloc', 4);
my $rc = $db->invoke('sqlite3_open', $cstr->(':memory:'), $pp_db);
die "open rc=$rc" unless $rc == 0;
my $dbh = $mem->i32_load($pp_db);

$rc = $db->invoke('sqlite3_exec', $dbh, $cstr->("create table t(a,b); insert into t values (1,'x'),(2,'y');"), 0, 0, 0);
die 'exec rc=' . $rc . ': ' . $read_cstr->($db->invoke('sqlite3_errmsg', $dbh)) unless $rc == 0;

my $pp_stmt = $db->invoke('sqlite3_malloc', 4);
$rc = $db->invoke('sqlite3_prepare_v2', $dbh, $cstr->('select a*10, b from t order by a desc'), 0xffffffff, $pp_stmt, 0);
die "prepare rc=$rc" unless $rc == 0;
my $stmt = $mem->i32_load($pp_stmt);

while ($db->invoke('sqlite3_step', $stmt) == 100) {
    my $n = $db->invoke('sqlite3_column_count', $stmt);
    print join('|', map { $read_cstr->($db->invoke('sqlite3_column_text', $stmt, $_)) } 0 .. $n - 1), "\n";
}
$db->invoke('sqlite3_finalize', $stmt);
$db->invoke('sqlite3_close', $dbh);
print "C-API-OK\n";
"#;

const PERL_LIBSQLITE3_FILE: &str = r#"
my $db = Libsqlite3->new({}, preopens => { '/db' => '{scratch}' });
$db->invoke('_initialize');
my $mem = $db->{memory};

my $read_cstr = sub {
    my ($ptr) = @_;
    return undef if $ptr == 0;
    my $end = index($mem->{data}, "\0", $ptr);
    return $mem->read_string($ptr, $end - $ptr);
};

my $cstr = sub {
    my ($s) = @_;
    my $b = $s . "\0";
    my $p = $db->invoke('sqlite3_malloc', length($b));
    $mem->init($p, $b, 0, length($b));
    return $p;
};

my $open_db = sub {
    my ($path) = @_;
    my $pp = $db->invoke('sqlite3_malloc', 4);
    my $rc = $db->invoke('sqlite3_open', $cstr->($path), $pp);
    die "open rc=$rc" unless $rc == 0;
    return $mem->i32_load($pp);
};

my $dbh = $open_db->('/db/data.db');
my $rc = $db->invoke('sqlite3_exec', $dbh, $cstr->("create table t(a,b); insert into t values (1,'x'),(2,'y');"), 0, 0, 0);
die 'exec rc=' . $rc . ': ' . $read_cstr->($db->invoke('sqlite3_errmsg', $dbh)) unless $rc == 0;
$db->invoke('sqlite3_close', $dbh);

$dbh = $open_db->('/db/data.db');
my $pp_stmt = $db->invoke('sqlite3_malloc', 4);
$rc = $db->invoke('sqlite3_prepare_v2', $dbh, $cstr->('select a*10, b from t order by a'), 0xffffffff, $pp_stmt, 0);
die "prepare rc=$rc" unless $rc == 0;
my $stmt = $mem->i32_load($pp_stmt);
while ($db->invoke('sqlite3_step', $stmt) == 100) {
    my $n = $db->invoke('sqlite3_column_count', $stmt);
    print join('|', map { $read_cstr->($db->invoke('sqlite3_column_text', $stmt, $_)) } 0 .. $n - 1), "\n";
}
$db->invoke('sqlite3_finalize', $stmt);
$db->invoke('sqlite3_close', $dbh);
print "FILE-OK\n";
"#;

const PERL_SQLITE3_CALLBACK: &str = r#"
my @rows;
my $mem;
my $host_row = sub {
    my ($argc, $argv_ptr) = @_;
    my @row;
    for my $i (0 .. $argc - 1) {
        my $p = $mem->i32_load($argv_ptr + $i * 4);
        if ($p == 0) {
            push @row, undef;
        } else {
            my $end = index($mem->{data}, "\0", $p);
            push @row, $mem->read_string($p, $end - $p);
        }
    }
    push @rows, \@row;
    return 0;
};

my $db = Sqlite3Binding->new({ 'env' => { 'host_row' => $host_row } });
$db->invoke('_initialize');
$mem = $db->{memory};

my $read_cstr = sub {
    my ($ptr) = @_;
    return undef if $ptr == 0;
    my $end = index($mem->{data}, "\0", $ptr);
    return $mem->read_string($ptr, $end - $ptr);
};

my $cstr = sub {
    my ($s) = @_;
    my $b = $s . "\0";
    my $p = $db->invoke('sqlite3_malloc', length($b));
    $mem->init($p, $b, 0, length($b));
    return $p;
};

my $pp_db = $db->invoke('sqlite3_malloc', 4);
my $rc = $db->invoke('sqlite3_open', $cstr->(':memory:'), $pp_db);
die "open rc=$rc" unless $rc == 0;
my $dbh = $mem->i32_load($pp_db);

$rc = $db->invoke('sqlite3_exec', $dbh, $cstr->("create table t(a,b); insert into t values (1,'x'),(2,'y'),(3,'z');"), 0, 0, 0);
die 'exec rc=' . $rc . ': ' . $read_cstr->($db->invoke('sqlite3_errmsg', $dbh)) unless $rc == 0;

$rc = $db->invoke('run_query', $dbh, $cstr->('select a, b from t where a >= 2 order by a'));
die "run_query rc=$rc" unless $rc == 0;
$db->invoke('sqlite3_close', $dbh);

print 'row: ', join('|', @$_), "\n" for @rows;
print "CALLBACK-OK\n";
"#;

/// libpcap BPF filter compilation: drive `compile_filter` on "tcp port 80" (DLT_EN10MB, snaplen 65535), then walk the serialized program `[u32 bf_len][bf_len x {u16 code; u8 jt; u8 jf; u32 k}]` in guest memory, printing each instruction as `code jt jf k`.
const PERL_PCAP_COMPILE: &str = r#"
my $inst = Libpcap->new({});
$inst->invoke('_initialize');
my $mem = $inst->{memory};

my $cstr = sub {
    my ($s) = @_;
    my $b = $s . "\0";
    my $p = $inst->invoke('malloc', length($b));
    $mem->init($p, $b, 0, length($b));
    return $p;
};

my $prog = $inst->invoke('compile_filter', $cstr->('tcp port 80'), 1, 65535);
die 'compile failed' unless $prog != 0;
my $n = $mem->i32_load($prog);
for my $i (0 .. $n - 1) {
    my $base = $prog + 4 + $i * 8;
    my $code = unpack('v', substr($mem->{data}, $base, 2));
    my $jt = unpack('C', substr($mem->{data}, $base + 2, 1));
    my $jf = unpack('C', substr($mem->{data}, $base + 3, 1));
    my $k = $mem->i32_load($base + 4);
    print "$code $jt $jf $k\n";
}
$inst->invoke('free', $prog);
print "BPF-OK\n";
"#;

/// tree-sitter JSON parse: drive `parse_source` on the fixed snippet `{"key": [1, true, null]}` and print the parse tree's S-expression (a malloc'd NUL-terminated C string) from guest memory.
const PERL_TREESITTER_PARSE: &str = r#"
my $inst = Treesitter->new({});
$inst->invoke('_initialize');
my $mem = $inst->{memory};

my $cstr = sub {
    my ($s) = @_;
    my $b = $s . "\0";
    my $p = $inst->invoke('malloc', length($b));
    $mem->init($p, $b, 0, length($b));
    return $p;
};

my $src = '{"key": [1, true, null]}';
my $r = $inst->invoke('parse_source', $cstr->($src), length($src));
die 'parse failed' unless $r != 0;
my $end = index($mem->{data}, "\0", $r);
print $mem->read_string($r, $end - $r), "\n";
$inst->invoke('free', $r);
print "TS-OK\n";
"#;

/// zeroperl Perl-5.42 eval (issue #67): instantiate the reactor with a
/// zero-returning `env.call_host_function` import stub (only invoked when the
/// guest registers host callbacks — this program registers none) and a
/// `/dev/null` preopen (`zeroperl_init` returns 1 without it), then
/// `_initialize` → `zeroperl_init` → `malloc` + copy a Perl program into guest
/// memory → `zeroperl_eval` → `zeroperl_flush`. Perl-on-Perl: the guest snippet
/// (a non-interpolating `<<'GUEST_PROGRAM'` heredoc, delimiter chosen not to
/// collide with either Perl layer) is byte-identical to the other backends'.
const PERL_ZEROPERL_EVAL: &str = r#"
my $inst = Zeroperl->new(
    { 'env' => { 'call_host_function' => sub { 0 } } },
    preopens => { '/dev/null' => '/dev/null' },
);
$inst->invoke('_initialize');
my $rc = $inst->invoke('zeroperl_init');
die "zeroperl_init rc=$rc" unless $rc == 0;
my $mem = $inst->{memory};

my $prog = <<'GUEST_PROGRAM';
my $s = "hello world 42";
if ($s =~ /(\w+)\s+(\w+)\s+(\d+)/) {
  printf("m=%s|%s|%d sum=%d\n", $1, $2, $3, $3 + 8);
}
GUEST_PROGRAM
my $bytes = $prog . "\0";
my $ptr = $inst->invoke('malloc', length($bytes));
$mem->init($ptr, $bytes, 0, length($bytes));
$inst->invoke('zeroperl_eval', $ptr, 0, 0, 0);
$inst->invoke('zeroperl_flush');
"#;

/// ExifTool on zeroperl (issue #70): the flattened `exiftool` CLI driver
/// (`{cache}/exiftool-lib`, preopened at `/work`) run on the same
/// `cache/zeroperl.wasm` reactor, whose SFS blob embeds the `Image::ExifTool`
/// module tree. Instantiated like [`PERL_ZEROPERL_EVAL`] plus the staged image
/// at `/img`. The guest driver snippet (identical bytes to the other backends')
/// overrides `CORE::GLOBAL::exit` to a `die` so ExifTool's terminal `exit`
/// unwinds back into `eval_pv` instead of tripping `proc_exit`, sets
/// `@ARGV`/`$0`, and `do`es the script; `zeroperl_flush` then pushes ExifTool's
/// buffered stdout out through fd 1.
const PERL_EXIFTOOL: &str = r#"
my $inst = Zeroperl->new(
    { 'env' => { 'call_host_function' => sub { 0 } } },
    preopens => {
        '/dev/null' => '/dev/null',
        '/work' => '{cache}/exiftool-lib',
        '/img' => '{scratch}',
    },
);
$inst->invoke('_initialize');
my $rc = $inst->invoke('zeroperl_init');
die "zeroperl_init rc=$rc" unless $rc == 0;
my $mem = $inst->{memory};

my $driver = <<'GUEST_PROGRAM';
BEGIN { *CORE::GLOBAL::exit = sub (;$) { die "zeroperl_exit\n" }; }
@ARGV = ('-S', '-Make', '-Model', '-DateTimeOriginal', '/img/exif_fixture.jpg');
$0 = '/work/exiftool';
do '/work/exiftool';
GUEST_PROGRAM
my $bytes = $driver . "\0";
my $ptr = $inst->invoke('malloc', length($bytes));
$mem->init($ptr, $bytes, 0, length($bytes));
$inst->invoke('zeroperl_eval', $ptr, 0, 0, 0);
$inst->invoke('zeroperl_flush');
"#;

/// DOOM (ADR-53): deterministic drive (synthetic clock, no input) dumping the framebuffer as a P6 PPM matching the wasmtime snapshot. `{ticks}`/`{clock_step}` filled by the runner.
const PERL_DOOM_FRAME_GLUE: &str = r#"my $frame = { off => undef, w => 0, h => 0 };
my $ms = 0;
my $doom = Doom->new({
    'console' => { 'onErrorMessage' => sub { }, 'onInfoMessage' => sub { } },
    'gameSaving' => {
        'sizeOfSaveGame' => sub { 0 },
        'readSaveGame' => sub { 0 },
        'writeSaveGame' => sub { $_[2] },
    },
    'runtimeControl' => { 'timeInMilliseconds' => sub { $ms += {clock_step}; return $ms; } },
    'ui' => { 'drawFrame' => sub { $frame->{off} = $_[0] } },
    'loading' => {
        'onGameInit' => sub { ($frame->{w}, $frame->{h}) = @_ },
        'wadSizes' => sub { },
        'readWads' => sub { },
    },
});
$doom->invoke('initGame');
$doom->invoke('tickGame') for 1 .. {ticks};
my ($w, $h) = ($frame->{w}, $frame->{h});
my $rgb = substr($doom->{memory}{data}, $frame->{off}, $w * $h * 4);
$rgb =~ s/(.)(.)(.)./$3$2$1/gs;    # memory is B,G,R,A; PPM wants R,G,B (alpha dropped)
binmode(STDOUT);
print "P6\n$w $h\n255\n";
print $rgb;
"#;

/// NES (issue #114, mirrors the DOOM glue above): load the pinned ROM into
/// `allocRom`'s buffer, tick `{frames}` times with no input, compose the frame
/// from agnes's palette-index screen buffer and its palette (issue #117; the
/// `& 0x3f` mask is load-bearing) and dump it as a P6 PPM matching the wasmtime
/// snapshot. `{rom}` (the cached ROM's host path) and `{frames}` filled by the
/// runner.
const PERL_NES_FRAME_GLUE: &str = r#"my $rom = do {
    local $/;
    open my $fh, '<:raw', "{rom}" or die $!;
    <$fh>;
};
my $nes = Nes->new({});
$nes->invoke('_initialize');
my $mem = $nes->{memory};
my $ptr = $nes->invoke('allocRom', length($rom));
$mem->init($ptr, $rom, 0, length($rom));
my $ok = $nes->invoke('initGame');
die "initGame failed: $ok" unless $ok == 1;
$nes->invoke('tickGame') for 1 .. {frames};
my $w = $nes->invoke('frameWidth');
my $h = $nes->invoke('frameHeight');
my $screen = substr($mem->{data}, $nes->invoke('screenOffset'), $w * $h);
my $palette = substr($mem->{data}, $nes->invoke('paletteOffset'), 64 * 4);
my @entries = map { substr($palette, $_ * 4, 3) } 0 .. 63;    # R,G,B; A dropped
my $rgb = join('', map { $entries[$_ & 0x3f] } unpack('C*', $screen));
binmode(STDOUT);
print "P6\n$w $h\n255\n";
print $rgb;
"#;

// --------------------------------------------------------------------- Multi-module drive glue.

/// Driver for the shared-table case: instantiate the exporter and the importer linked against it, then print `call0` (call_indirect through the shared table -> 42).
const PERL_SHARED_TABLE_GLUE: &str = r#"my $a = TableExp->new({});
my $b = TableImp->new({ 'a' => $a });
print $b->invoke('call0'), "\n";
"#;

/// Driver for the Embedded-coexistence case: two independently converted packages, each with its own prefix-namespaced runtime (`Alpha::Rt`, `Beta::Rt`), living in one interpreter. The trap raised by one must be its own runtime's class, not the other's.
const PERL_EMBEDDED_COEXIST_GLUE: &str = r#"my $a = Alpha->new({});
my $b = Beta->new({});
print $a->invoke('div', 7, 2), "\n";
print $b->invoke('div', 0xfffffff9, 2), "\n";
eval { $a->invoke('div', 1, 0); };
my $e = $@;
print((ref($e) && $e->isa('Alpha::Rt::Trap') && !$e->isa('Beta::Rt::Trap')) ? 'distinct-rt' : 'same-rt', "\n");
print "trapped\n" if ref($e) && $e->isa('Alpha::Rt::Trap');
"#;

// --------------------------------------------------------------------- Suite wiring (ADR-27): each per-case macro invocation declares participation.

dewasm_test_helper::library_add_e2e!(Perl, PERL_ADD_GLUE);
dewasm_test_helper::wasi_import_override_e2e!(Perl, PERL_OVERRIDE_GLUE);
dewasm_test_helper::custom_wasi_provider_e2e!(Perl, PERL_CUSTOM_PROVIDER_GLUE);
dewasm_test_helper::partial_override_e2e!(Perl, PERL_PARTIAL_OVERRIDE_GLUE);
dewasm_test_helper::stdio_capture_e2e!(Perl, PERL_STDIO_CAPTURE_GLUE);

dewasm_test_helper::wasi_suite!(Perl, Stdio);
dewasm_test_helper::wasi_suite!(Perl, ArgsEnv);
dewasm_test_helper::wasi_suite!(Perl, Poll);
dewasm_test_helper::wasi_suite!(Perl, Fs, PERL_FS_GLUE);
dewasm_test_helper::wasi_root_containment_e2e!(Perl, PERL_CONTAINMENT_GLUE);
dewasm_test_helper::standalone_dir_e2e!(Perl);
// Perl recursion is heap-allocated (no host stack to overflow, ADR-55); the case pins that the entrypoint still surfaces proc_exit(42) through deep guest recursion.
dewasm_test_helper::deep_recursion_e2e!(Perl);

dewasm_test_helper::cowsay_args_e2e!(Perl);
dewasm_test_helper::cowsay_stdin_e2e!(Perl);
dewasm_test_helper::qjs_eval_e2e!(Perl);
dewasm_test_helper::sqlite3_shell_e2e!(Perl);
dewasm_test_helper::gzip_e2e!(Perl);

dewasm_test_helper::qjs_file_io_e2e!(Perl, PERL_QJS_FILE_IO_GLUE);
dewasm_test_helper::sqlite3_shell_dbfile_e2e!(Perl, PERL_SQLITE3_SHELL_GLUE);
dewasm_test_helper::rg_search_e2e!(Perl, PERL_RG_SEARCH_GLUE);
dewasm_test_helper::cpython_hello_e2e!(Perl, PERL_CPYTHON_GLUE);
// Ultra tier (ADR-48): measured ~57s locally (CRuby-on-Perl), which crosses the ~1-minute CI-runner line the other backends' cruby cases stay under. The packed variant is the same interpreter plus the wizer-embedded stdlib (ADR-61), so it inherits the tier.
dewasm_test_helper::cruby_hello_e2e!(Perl, PERL_CRUBY_GLUE, ultra);
dewasm_test_helper::cruby_packed_hello_e2e!(Perl, ultra);
dewasm_test_helper::qjs_repl_pty_e2e!(Perl);

dewasm_test_helper::libsqlite3_c_api_e2e!(Perl, PERL_LIBSQLITE3_MEM);
dewasm_test_helper::sqlite3_file_c_api_e2e!(Perl, PERL_LIBSQLITE3_FILE);
dewasm_test_helper::sqlite3_callback_binding_e2e!(Perl, PERL_SQLITE3_CALLBACK);
dewasm_test_helper::pcap_compile_e2e!(Perl, PERL_PCAP_COMPILE);
dewasm_test_helper::treesitter_parse_e2e!(Perl, PERL_TREESITTER_PARSE);
dewasm_test_helper::zeroperl_eval_e2e!(Perl, PERL_ZEROPERL_EVAL);
// Ultra tier (ADR-48): measured ~75s locally (ExifTool-on-zeroperl-on-Perl), well past the ~1-minute CI-runner line; the zeroperl_eval case above (~7s: same convert + host-perl compile, tiny guest program) pins the embedding path at the slow tier.
dewasm_test_helper::exiftool_extract_e2e!(Perl, PERL_EXIFTOOL, ultra);

// Slow tier like Ruby/Python (ADR-53): measured ~10s locally (convert + initGame + 2 ticks), nowhere near the ~1-minute ultra line.
dewasm_test_helper::doom_frame_e2e!(Perl, PERL_DOOM_FRAME_GLUE);
dewasm_test_helper::nes_frame_e2e!(Perl, PERL_NES_FRAME_GLUE);

dewasm_test_helper::shared_table_e2e!(Perl, PERL_SHARED_TABLE_GLUE);
dewasm_test_helper::embedded_coexist_e2e!(Perl, PERL_EMBEDDED_COEXIST_GLUE);
