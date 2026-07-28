module SQLite3
  class Database
    include Pragmas

    class << self
      def quote(string)
        string.to_s.gsub("'", "''")
      end

      def open(*args, &block)
        new(*args, &block)
      end
    end

    attr_reader :driver, :handle, :results_as_hash, :default_transaction_mode

    def initialize(file, options = {}, &block)
      file = file.to_path if file.respond_to?(:to_path)
      file = file.to_s

      @results_as_hash = options.fetch(:results_as_hash, false)
      @default_transaction_mode = options.fetch(:default_transaction_mode, :deferred)
      @readonly = options[:readonly] ? true : false
      @closed = false
      @statements = {}

      flags = options[:flags]
      flags ||=
        if options[:readonly]
          Constants::Open::READONLY
        elsif options[:readwrite]
          Constants::Open::READWRITE
        else
          Constants::Open::READWRITE | Constants::Open::CREATE
        end
      @readonly = (flags & Constants::Open::READONLY) != 0

      # The wasm guest sees the host filesystem 1:1 (the driver preopens "/"),
      # but its cwd is not the Ruby process's cwd — absolutize relative paths
      # so both agree on where the database lives.
      @path =
        if file.empty? || file == ":memory:" || file.start_with?("file:")
          file
        else
          File.expand_path(file)
        end

      @driver = WasmDriver.new
      open_v2(@path, flags)

      if options[:extensions] && !Array(options[:extensions]).empty?
        raise NotImplementedError, "sqlite extensions are not supported by the dewasm shim"
      end
      # `strict:` (double-quoted-string rejection) needs sqlite3_db_config,
      # a varargs API we don't export; ignore it rather than fail closed.

      if block
        begin
          yield self
        ensure
          close
        end
      end
    end

    def prepare(sql)
      stmt = Statement.new(self, sql)
      if block_given?
        begin
          yield stmt
        ensure
          stmt.close
        end
      else
        stmt
      end
    end

    def execute(sql, bind_vars = [], &block)
      prepare(sql) do |stmt|
        stmt.bind_params(bind_vars)
        rows = stmt.to_a
        rows = rows.map { |row| stmt.columns.zip(row).to_h } if @results_as_hash
        if block
          rows.each(&block)
        else
          rows
        end
      end
    end

    def query(sql, bind_vars = [])
      stmt = prepare(sql)
      stmt.bind_params(bind_vars)
      if block_given?
        begin
          yield stmt
        ensure
          stmt.close
        end
      else
        stmt
      end
    end

    def get_first_row(sql, *bind_vars)
      execute(sql, bind_vars).first
    end

    def get_first_value(sql, *bind_vars)
      prepare(sql) do |stmt|
        stmt.bind_params(bind_vars)
        row = stmt.step
        return row&.first
      end
    end

    def execute_batch(sql, bind_vars = [])
      remaining = sql.to_s
      until remaining.strip.empty?
        prepare(remaining) do |stmt|
          remaining = stmt.remainder
          stmt.bind_params(bind_vars)
          nil while stmt.step
        end
      end
      nil
    end

    # Rails' multi-statement path (fixtures, structure load). The real gem
    # routes this through sqlite3_exec; the return value is discarded by
    # Rails, so plain sequential execution is enough.
    def execute_batch2(sql)
      execute_batch(sql)
      []
    end

    def changes
      @driver.call("sqlite3_changes", @handle)
    end

    def total_changes
      @driver.call("sqlite3_total_changes", @handle)
    end

    def last_insert_row_id
      @driver.to_s64(@driver.call("sqlite3_last_insert_rowid", @handle))
    end

    def transaction_active?
      @driver.call("sqlite3_get_autocommit", @handle).zero?
    end

    def transaction(mode = nil)
      execute("begin #{mode || @default_transaction_mode} transaction")
      return true unless block_given?
      begin
        result = yield self
        commit
        result
      rescue StandardError
        rollback rescue nil
        raise
      end
    end

    def commit
      execute("commit transaction")
      true
    end

    def rollback
      execute("rollback transaction")
      true
    end

    def readonly?
      @readonly
    end

    def closed?
      @closed
    end

    def close
      return if @closed
      @statements.keys.each(&:close)
      @driver.call("sqlite3_close_v2", @handle)
      @closed = true
      self
    end

    def filename(db_name = "main")
      @path
    end

    def encoding
      Encoding.find(get_first_value("PRAGMA encoding"))
    end

    def busy_timeout(ms)
      check(@driver.call("sqlite3_busy_timeout", @handle, ms.to_i))
    end
    alias busy_timeout= busy_timeout

    # The real gem implements this as a Ruby busy_handler retry loop; a
    # guest->host callback is not available here, so sqlite's built-in
    # timeout handler stands in — same observable outcome (SQLITE_BUSY after
    # ~ms milliseconds → BusyException → ActiveRecord::StatementTimeout).
    def busy_handler_timeout=(ms)
      busy_timeout(ms)
    end

    def complete?(sql)
      @driver.with_cstr(sql) { |p| @driver.call("sqlite3_complete", p) } != 0
    end

    def load_extension(_path)
      raise NotImplementedError, "sqlite extensions are not supported by the dewasm shim"
    end

    # --- internal, used by Statement ---------------------------------------

    def register_statement(stmt)
      @statements[stmt] = true
    end

    def unregister_statement(stmt)
      @statements.delete(stmt)
    end

    def check(rc, sql: nil)
      return rc if rc == Constants::ErrorCode::OK ||
                   rc == Constants::ErrorCode::ROW ||
                   rc == Constants::ErrorCode::DONE
      raise error_for(rc, sql: sql)
    end

    def error_for(rc, sql: nil)
      extended = @driver.call("sqlite3_extended_errcode", @handle)
      message = @driver.read_cstr(@driver.call("sqlite3_errmsg", @handle))
      SQLite3.exception_for(rc, message, extended_code: extended.zero? ? rc : extended, sql: sql)
    end

    private

    def open_v2(path, flags)
      pp_db = @driver.malloc(4)
      path_ptr = @driver.cstr_in(path)
      begin
        rc = @driver.call("sqlite3_open_v2", path_ptr, pp_db, flags, 0)
        @handle = @driver.mem.i32_load(pp_db)
        unless rc.zero?
          message =
            if @handle.zero?
              "unable to open database file #{path}"
            else
              @driver.read_cstr(@driver.call("sqlite3_errmsg", @handle))
            end
          @driver.call("sqlite3_close_v2", @handle) unless @handle.zero?
          raise SQLite3.exception_for(rc, message)
        end
      ensure
        @driver.free(path_ptr)
        @driver.free(pp_db)
      end
    end
  end
end
