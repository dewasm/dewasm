# A failed import resolution at instantiation time (missing import, or one of the wrong kind).
# It is kept distinct from Trap and from plain Ruby errors.
# So a harness can tell "the module failed to link" apart from "the module linked and then crashed".
class LinkError < StandardError; end
