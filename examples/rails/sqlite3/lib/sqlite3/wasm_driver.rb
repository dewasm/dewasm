require_relative "sqlite3_wasm"

module SQLite3
  # Low-level bridge to the dewasm-generated Sqlite3Wasm module. One driver
  # (one wasm instance, one linear memory, one guest heap) per Database, so a
  # Rails connection pool gets naturally isolated instances; the mutex guards
  # against interleaved calls into the same guest from multiple Ruby threads.
  class WasmDriver
    SQLITE_TRANSIENT = 0xffffffff # -1: sqlite copies the buffer before returning
    U64_MASK = (1 << 64) - 1

    attr_reader :mem

    def initialize
      # Preopening "/" at "/" makes guest paths identical to host paths, so
      # the database file lands wherever Rails configured it (ADR-14 sandbox
      # caveats accepted — this is a demo embedding, not a sandbox).
      @mod = Sqlite3Wasm.new({}, preopens: { "/" => "/" })
      @mod.invoke("_initialize")
      @mem = @mod.memory
      @mutex = Mutex.new
    end

    def call(name, *args)
      @mutex.synchronize { @mod.invoke(name, *args) }
    end

    # --- guest heap helpers -------------------------------------------------

    def malloc(size)
      p = call("sqlite3_malloc", size)
      raise MemoryException, "sqlite3_malloc(#{size}) failed" if p.zero?
      p
    end

    def free(ptr)
      call("sqlite3_free", ptr) unless ptr.zero?
    end

    # Copy a Ruby string into the guest as NUL-terminated UTF-8. Caller frees.
    def cstr_in(str)
      bytes = str.to_s.encode(Encoding::UTF_8).b
      p = malloc(bytes.bytesize + 1)
      @mem.init(p, "#{bytes}\0", 0, bytes.bytesize + 1)
      p
    end

    # Copy raw bytes (no terminator) into the guest. Caller frees.
    # Returns [ptr, bytesize]; ptr is 0 only for the empty string, in which
    # case a 1-byte allocation still gives sqlite a non-NULL base pointer.
    def bytes_in(str)
      bytes = str.to_s.b
      size = bytes.bytesize
      p = malloc(size.zero? ? 1 : size)
      @mem.init(p, bytes, 0, size) unless size.zero?
      [p, size]
    end

    def with_cstr(str)
      p = cstr_in(str)
      yield p
    ensure
      free(p) if p
    end

    # --- guest memory readers ----------------------------------------------

    def read_bytes(ptr, len)
      return "" if len.zero?
      @mem.read_string(ptr, len).b
    end

    # NUL-terminated string; scans in chunks rather than per byte.
    def read_cstr(ptr)
      return nil if ptr.zero?
      buf = @mem.buffer
      limit = buf.size
      chunks = (+"").b
      pos = ptr
      while pos < limit
        len = [256, limit - pos].min
        chunk = buf.get_string(pos, len)
        if (nul = chunk.index("\0"))
          return (chunks << chunk[0, nul]).force_encoding(Encoding::UTF_8)
        end
        chunks << chunk
        pos += len
      end
      chunks.force_encoding(Encoding::UTF_8)
    end

    # A 4-byte out-parameter slot: yields the pointer, returns the i32 read
    # back from it after the block.
    def with_out_i32
      p = malloc(4)
      begin
        yield p
        @mem.i32_load(p)
      ensure
        free(p)
      end
    end

    # --- ADR-2 masked-unsigned <-> Ruby signed ------------------------------

    def to_s64(v)
      v >= 0x8000_0000_0000_0000 ? v - (1 << 64) : v
    end

    def to_u64(v)
      v & U64_MASK
    end
  end
end
