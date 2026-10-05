# Constructs the decompiler must keep: a hidden python block in another
# store, an ATL warp, and a screen id. Compiled copies of the main fixture
# live next to this file; this source is the reference for those shapes.

init python hide in prefs:
    seen = False

image logo:
    warp slow
    xpos 0.5

screen greeting:
    text "Hello" id "greeting"
