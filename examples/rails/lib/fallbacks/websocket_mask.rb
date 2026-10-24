# frozen_string_literal: true

# Found ahead of the C extension on the load path; `websocket-driver` falls back to Ruby on it.
raise LoadError, "websocket_mask: the pure Ruby WebSocket::Mask is used instead"
