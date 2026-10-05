# The bundled demo. Each problem is deliberate, so Problems, the map and
# replay have something to show on a machine with no Ren'Py SDK.
# A repeated label is not one of them: Ren'Py refuses to launch the game.
# The undefined speaker and the jump to a missing label live in `orphan`,
# which nothing reaches, so playing either choice finishes.

define e = Character("Eileen")

label start:
    e "Welcome to the Ren'Inspector demo."
    show eileen happy
    "The line above shows an image that was never defined."
    menu:
        "Go to the cafe.":
            jump cafe
        "Head home.":
            jump home

label cafe:
    e "The project map links the start to this label."
    e "That's the end of the demo."
    return

label home:
    e "This choice leads somewhere too."
    return

label orphan:
    "Nothing jumps or falls through to this label."
    nobody "This speaker was never defined."
    jump nowhere
