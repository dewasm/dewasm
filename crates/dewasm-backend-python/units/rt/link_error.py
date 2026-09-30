# A failed import resolution at instantiation time (missing import, or one of the wrong kind).
# It is kept distinct from Trap and from plain Python errors.
class LinkError(Exception):
    pass
