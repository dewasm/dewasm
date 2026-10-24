# frozen_string_literal: true

# Found ahead of the C extension on the load path; `racc` falls back to its Ruby runtime on it.
raise LoadError, "racc/cparse: the Ruby racc runtime is used instead"
