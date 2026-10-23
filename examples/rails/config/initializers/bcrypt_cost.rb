# frozen_string_literal: true

# The converted `bcrypt` runs about 60 times slower than the native one under YJIT.
# At the default cost of 12, one sign-in takes about 19 seconds, so this example uses cost 5.
# That is 2 to the 5th rounds of Blowfish: about 0.2 seconds per sign-in here.
BCrypt::Engine.cost = 5
