# A failed import resolution at instantiation time (missing import, or one
# of the wrong kind), kept distinct from Trap and from plain Python errors.
class LinkError(Exception):
    pass
