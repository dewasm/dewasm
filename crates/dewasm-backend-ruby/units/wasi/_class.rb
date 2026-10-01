# requires: wasi/rights
ERRNO_SUCCESS = 0
ERRNO_BADF = 8
ERRNO_INVAL = 28
ERRNO_IO = 29
ERRNO_NOSYS = 52
ERRNO_NOTSUP = 58
ERRNO_SPIPE = 70
# NOTCAPABLE lives in this always-bundled prelude, not `errno_fs`.
# That is because the per-descriptor rights model raises it from the stdio-core `fd_*` units too.
# It is not raised only from the `path_*` units that pull in `errno_fs`.
ERRNO_NOTCAPABLE = 76

# A directory descriptor: either a preopen or a directory the guest opened itself via `path_open`.
# A preopen sets `preopen_name` to the guest-visible path passed in `preopens:`.
# A directory the guest opened has `preopen_name` `nil`.
# `entries` is the `fd_readdir` listing cache, populated lazily.
# Kept in the prelude (rather than with the rest of the file system logic) because of `initialize`.
# It builds one per preopen unconditionally.
# So it must be available for any WASI import, not only for a system call on the file system.
WasiDir = Struct.new(:host_path, :preopen_name, :entries)

attr_reader :memory

def initialize(args: [], env: {}, preopens: {})
  @args = args.map(&:to_s)
  @env = env.map { |k, v| "#{k}=#{v}" }
  @fds = { 0 => $stdin, 1 => $stdout, 2 => $stderr }
  # Per-descriptor capability metadata: `fd => [rights_base, rights_inheriting, fdflags, filetype]`.
  # `filetype` is what `fd_fdstat_get` reports, filled in on its first query and `nil` until then.
  # An open descriptor's file type cannot change while it is open.
  # This metadata travels with its `@fds` entry: `fd_renumber` moves both.
  # A closed descriptor is never brought back.
  # So the memoized answer cannot live longer than the descriptor it describes.
  # stdio is seeded all-rights: it is never rights-tested and must stay readable/writable.
  # Preopens are seeded likewise, so a real embedder keeps unrestricted access.
  # `path_open` derives the narrowed rights from them.
  @fd_meta = {
    0 => [Rt::M64, Rt::M64, 0, nil],
    1 => [Rt::M64, Rt::M64, 0, nil],
    2 => [Rt::M64, Rt::M64, 0, nil],
  }
  # The stdio special-cases key on the objects captured here, the ones the `@fds` table holds.
  # Those are SPIPE on `seek`/`tell`/`pread`/`pwrite`, and no close.
  # They do not key on whatever the globals point at when a system call runs.
  @std_ios = [$stdin, $stdout, $stderr].freeze
  next_fd = 3
  preopens.each do |guest, host|
    # The host path must resolve, but need not be a directory.
    # A single-file preopen is accepted.
    # An example is "/dev/null" for the `zeroperl` reactor's initialization probe.
    # The guest resolves it as the preopen root itself.
    real = begin
      File.realpath(host)
    rescue SystemCallError => e
      raise ArgumentError, "preopen #{guest.inspect} => #{host.inspect}: #{e.message}"
    end
    @fds[next_fd] = WasiDir.new(real, guest, nil)
    # A preopen is a directory, so its base is the directory-rights set (no FD_WRITE etc.).
    # Its inheriting rights carry the full file-rights set.
    # So guest-opened files under it get real read/write capability.
    # `root_directory()` in the testsuite reopens the preopen with exactly these.
    # Seeding all-of-M64 here would wrongly hand a directory the write right.
    # That would make the reopen fail EISDIR.
    @fd_meta[next_fd] = [DIR_BASE_RIGHTS, DIR_INHERITING_RIGHTS, 0, nil]
    next_fd += 1
  end
  @next_fd = next_fd
  $stdout.binmode
  $stderr.binmode
  $stdin.binmode
end

def attach(instance)
  @memory = instance.memory
end
