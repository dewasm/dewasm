# frozen_string_literal: true

# `websocket-driver` requires `websocket_mask` (its C extension); on a `LoadError` it uses Ruby.
# This file, ahead of the gem on the load path, is that `LoadError`.
raise LoadError, "websocket_mask: the pure Ruby WebSocket::Mask is used instead"
