module SQLite3
  class Statement
    include Enumerable

    ROW = Constants::ErrorCode::ROW
    DONE = Constants::ErrorCode::DONE

    # SQL left uncompiled after the first statement (sqlite3_prepare_v2 tail);
    # execute_batch2 consumes it statement by statement.
    attr_reader :remainder

    def initialize(db, sql)
      raise ArgumentError, "prepare called on a closed database" if db.closed?

      sql = sql.to_s.encode(Encoding::UTF_8)
      @db = db
      @drv = db.driver
      @closed = false
      @done = false
      @stmt = 0
      @remainder = ""

      drv = @drv
      sql_ptr = drv.cstr_in(sql)
      pp_stmt = drv.malloc(4)
      pp_tail = drv.malloc(4)
      begin
        rc = drv.call("sqlite3_prepare_v2", db.handle, sql_ptr, sql.bytesize + 1, pp_stmt, pp_tail)
        db.check(rc, sql: sql)
        @stmt = drv.mem.i32_load(pp_stmt)
        tail = drv.mem.i32_load(pp_tail)
        # The tail points into sql_ptr's buffer: read it before the ensure frees it.
        @remainder = tail.zero? ? "" : drv.read_cstr(tail)
      ensure
        drv.free(sql_ptr)
        drv.free(pp_stmt)
        drv.free(pp_tail)
      end

      # Whitespace/comment-only SQL compiles to no statement (@stmt == 0);
      # such a Statement steps straight to done, like the real gem's.
      @column_count = @stmt.zero? ? 0 : drv.call("sqlite3_column_count", @stmt)
      @db.register_statement(self)

      if block_given?
        begin
          yield self
        ensure
          close
        end
      end
    end

    attr_reader :column_count

    def step
      must_be_open!
      return nil if @done || @stmt.zero?
      rc = @drv.call("sqlite3_step", @stmt)
      case rc
      when ROW
        read_row
      when DONE
        @done = true
        nil
      else
        # Reset first (like the C gem) so the statement stays reusable, then
        # raise with the message sqlite left on the connection.
        @drv.call("sqlite3_reset", @stmt)
        raise @db.error_for(rc)
      end
    end

    def each
      while (row = step)
        yield row
      end
      self
    end

    def bind_params(*bind_vars)
      index = 1
      bind_vars.flatten.each do |var|
        if var.is_a?(Hash)
          var.each { |key, value| bind_param(key, value) }
        else
          bind_param(index, var)
          index += 1
        end
      end
    end

    def bind_param(key, value)
      must_be_open!
      return if @stmt.zero?
      index = key.is_a?(Integer) ? key : named_param_index(key)
      rc =
        case value
        when nil
          @drv.call("sqlite3_bind_null", @stmt, index)
        when Integer
          if value.bit_length < 64
            @drv.call("sqlite3_bind_int64", @stmt, index, @drv.to_u64(value))
          else
            @drv.call("sqlite3_bind_double", @stmt, index, value.to_f)
          end
        when Float
          @drv.call("sqlite3_bind_double", @stmt, index, value)
        when String
          bind_string(index, value)
        else
          raise "can't prepare #{value.class}"
        end
      @db.check(rc)
    end

    def reset!
      must_be_open!
      @drv.call("sqlite3_reset", @stmt) unless @stmt.zero?
      @done = false
      self
    end

    def clear_bindings!
      must_be_open!
      @drv.call("sqlite3_clear_bindings", @stmt) unless @stmt.zero?
      self
    end

    def done?
      @done
    end

    def closed?
      @closed
    end

    def close
      return self if @closed
      @closed = true
      @db.unregister_statement(self)
      @drv.call("sqlite3_finalize", @stmt) unless @stmt.zero?
      self
    end

    def columns
      @columns ||= (0...@column_count).map do |i|
        @drv.read_cstr(@drv.call("sqlite3_column_name", @stmt, i)).freeze
      end
    end

    def types
      @types ||= (0...@column_count).map do |i|
        decl = @drv.read_cstr(@drv.call("sqlite3_column_decltype", @stmt, i))
        decl&.downcase
      end
    end

    private

    def must_be_open!
      raise SQLite3::Exception, "cannot use a closed statement" if @closed
    end

    def named_param_index(key)
      name = key.to_s
      name = ":#{name}" unless name.start_with?(":", "@", "$")
      index = @drv.with_cstr(name) do |p|
        @drv.call("sqlite3_bind_parameter_index", @stmt, p)
      end
      raise SQLite3::Exception, "no such bind parameter '#{name}'" if index.zero?
      index
    end

    def bind_string(index, value)
      if value.encoding == Encoding::ASCII_8BIT
        ptr, size = @drv.bytes_in(value)
        begin
          @drv.call("sqlite3_bind_blob", @stmt, index, ptr, size, WasmDriver::SQLITE_TRANSIENT)
        ensure
          @drv.free(ptr)
        end
      else
        utf8 = value.encode(Encoding::UTF_8)
        ptr, size = @drv.bytes_in(utf8)
        begin
          @drv.call("sqlite3_bind_text", @stmt, index, ptr, size, WasmDriver::SQLITE_TRANSIENT)
        ensure
          @drv.free(ptr)
        end
      end
    end

    def read_row
      (0...@column_count).map do |i|
        case @drv.call("sqlite3_column_type", @stmt, i)
        when Constants::ColumnType::INTEGER
          @drv.to_s64(@drv.call("sqlite3_column_int64", @stmt, i))
        when Constants::ColumnType::FLOAT
          @drv.call("sqlite3_column_double", @stmt, i)
        when Constants::ColumnType::TEXT
          len = @drv.call("sqlite3_column_bytes", @stmt, i)
          ptr = @drv.call("sqlite3_column_text", @stmt, i)
          @drv.read_bytes(ptr, len).force_encoding(Encoding::UTF_8).freeze
        when Constants::ColumnType::BLOB
          len = @drv.call("sqlite3_column_bytes", @stmt, i)
          ptr = @drv.call("sqlite3_column_blob", @stmt, i)
          @drv.read_bytes(ptr, len).freeze
        else
          nil
        end
      end
    end
  end
end
