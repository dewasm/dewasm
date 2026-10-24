# frozen_string_literal: true

# At the default cost of 12, one sign-in on the converted `bcrypt` takes about 19 seconds.
BCrypt::Engine.cost = 5
