# The sqlite3 gem's exception hierarchy, mapped from SQLite primary result codes.
# Rails rescues several of these by class (BusyException,
# ConstraintException, ...), so the class-per-code shape must match the real gem even though the messages come from our own sqlite3_errmsg round-trip.
module SQLite3
  class Exception < ::StandardError
    attr_reader :code
    # The real gem exposes the SQL and error offset on newer versions; Rails feature-detects them, so plain readers returning nil-ish defaults are enough to keep it happy.
    attr_reader :sql, :sql_offset

    def initialize(message = nil, code: nil, sql: nil, sql_offset: nil)
      super(message)
      @code = code
      @sql = sql
      @sql_offset = sql_offset || -1
    end
  end

  class SQLException < Exception; end
  class InternalException < Exception; end
  class PermissionException < Exception; end
  class AbortException < Exception; end
  class BusyException < Exception; end
  class LockedException < Exception; end
  class MemoryException < Exception; end
  class ReadOnlyException < Exception; end
  class InterruptException < Exception; end
  class IOException < Exception; end
  class CorruptException < Exception; end
  class NotFoundException < Exception; end
  class FullException < Exception; end
  class CantOpenException < Exception; end
  class ProtocolException < Exception; end
  class EmptyException < Exception; end
  class SchemaChangedException < Exception; end
  class TooBigException < Exception; end
  class ConstraintException < Exception; end
  class MismatchException < Exception; end
  class MisuseException < Exception; end
  class UnsupportedException < Exception; end
  class AuthorizationException < Exception; end
  class FormatException < Exception; end
  class RangeException < Exception; end
  class NotADatabaseException < Exception; end

  EXCEPTION_FOR_CODE = {
    1 => SQLException,
    2 => InternalException,
    3 => PermissionException,
    4 => AbortException,
    5 => BusyException,
    6 => LockedException,
    7 => MemoryException,
    8 => ReadOnlyException,
    9 => InterruptException,
    10 => IOException,
    11 => CorruptException,
    12 => NotFoundException,
    13 => FullException,
    14 => CantOpenException,
    15 => ProtocolException,
    16 => EmptyException,
    17 => SchemaChangedException,
    18 => TooBigException,
    19 => ConstraintException,
    20 => MismatchException,
    21 => MisuseException,
    22 => UnsupportedException,
    23 => AuthorizationException,
    24 => FormatException,
    25 => RangeException,
    26 => NotADatabaseException,
  }.freeze

  def self.exception_for(primary_code, message, extended_code: nil, sql: nil)
    klass = EXCEPTION_FOR_CODE.fetch(primary_code & 0xff, Exception)
    klass.new(message, code: extended_code || primary_code, sql: sql)
  end
end
