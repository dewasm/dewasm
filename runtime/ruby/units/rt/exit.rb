class Exit < StandardError
  attr_reader :code

  def initialize(code)
    @code = code
    super("proc_exit(#{code})")
  end
end
