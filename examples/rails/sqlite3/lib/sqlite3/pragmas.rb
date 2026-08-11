module SQLite3
  # Pragma accessors as *real* methods on a named module: Rails introspects
  # `SQLite3::Pragmas.method_defined?("#{pragma}=")` before public_send-ing the setter, so method_missing tricks would not survive.
  module Pragmas
    NAMES = %w[
      analysis_limit application_id auto_vacuum automatic_index busy_timeout
      cache_size cache_spill checkpoint_fullfsync defer_foreign_keys encoding
      foreign_keys freelist_count fullfsync hard_heap_limit
      ignore_check_constraints journal_mode journal_size_limit
      legacy_alter_table locking_mode max_page_count mmap_size page_count
      page_size query_only read_uncommitted recursive_triggers
      reverse_unordered_selects secure_delete short_column_names soft_heap_limit
      synchronous temp_store threads trusted_schema user_version
      wal_autocheckpoint writable_schema
    ].freeze

    NAMES.each do |pragma|
      define_method(pragma) { get_pragma(pragma) }
      define_method("#{pragma}=") { |value| set_pragma(pragma, value) }
    end

    def get_pragma(name)
      get_first_value("PRAGMA #{name}")
    end

    def set_pragma(name, value)
      literal =
        case value
        when true then "1"
        when false then "0"
        when nil then "NULL"
        when Integer then value.to_s
        else
          s = value.to_s
          /\A[A-Za-z0-9_.-]+\z/.match?(s) ? s : "'#{Database.quote(s)}'"
        end
      execute("PRAGMA #{name} = #{literal}")
    end
  end
end
