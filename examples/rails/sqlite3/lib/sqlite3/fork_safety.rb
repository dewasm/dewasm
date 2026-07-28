module SQLite3
  # The real gem tracks C-level connection handles across fork() and closes
  # the writable ones in the child. Our connections are pure Ruby state in a
  # copied linear memory, so there is nothing process-level to protect; the
  # module exists because Rails calls suppress_warnings! at require time.
  module ForkSafety
    def self.suppress_warnings!
    end

    def self.discard
    end
  end
end
