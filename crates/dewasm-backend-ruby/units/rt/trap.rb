class Trap < StandardError; end

def trap(message)
  raise Trap, message
end
