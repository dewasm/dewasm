module SQLite3
  module Constants
    # SQLITE_OPEN_* flags. Rails only reads Open::SHAREDCACHE (to warn about
    # shared-cache configs), but the full set keeps user `flags:` configs
    # working.
    module Open
      READONLY = 0x00000001
      READWRITE = 0x00000002
      CREATE = 0x00000004
      DELETEONCLOSE = 0x00000008
      EXCLUSIVE = 0x00000010
      AUTOPROXY = 0x00000020
      URI = 0x00000040
      MEMORY = 0x00000080
      MAIN_DB = 0x00000100
      TEMP_DB = 0x00000200
      TRANSIENT_DB = 0x00000400
      MAIN_JOURNAL = 0x00000800
      TEMP_JOURNAL = 0x00001000
      SUBJOURNAL = 0x00002000
      SUPER_JOURNAL = 0x00004000
      MASTER_JOURNAL = 0x00004000
      NOMUTEX = 0x00008000
      FULLMUTEX = 0x00010000
      SHAREDCACHE = 0x00020000
      PRIVATECACHE = 0x00040000
      WAL = 0x00080000
      NOFOLLOW = 0x01000000
    end

    module ErrorCode
      OK = 0
      ROW = 100
      DONE = 101
    end

    module ColumnType
      INTEGER = 1
      FLOAT = 2
      TEXT = 3
      BLOB = 4
      NULL = 5
    end
  end
end
