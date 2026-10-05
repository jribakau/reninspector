# The bundled demo. Each problem is deliberate, so Problems, the map and
# replay have something to show on a machine with no Ren'Py SDK.

define e = Character("Eileen")

label start:
    e "Welcome to the Ren'Inspector demo."
    show eileen happy
    "The line above shows an image that was never defined."
    menu:
        "Go to the cafe.":
            jump cafe
        "This choice goes nowhere.":
            jump nowhere

label cafe:
    e "The project map links the start to this label."
    nobody "This speaker was never defined."
    return

label orphan:
    "Nothing jumps or falls through to this label."

label start:
    "A second label with the same name."
