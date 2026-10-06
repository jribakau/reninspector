# A small story for the map: jumps, calls that return, a shared shop,
# a screen full of buttons, and menu conditions.

define karma = 6
define energy = 2

label start:
    scene bg room
    "Morning. The town is already awake."
    call screen town
    jump morning

label morning:
    menu:
        "Go to the shop" if karma > 5:
            jump shop_visit
        "Stay home" if energy > 1:
            jump home
        "Wander the park":
            jump park

label shop_visit:
    "The bell over the door rings."
    call shop
    "Back on the street, parcels in hand."
    jump evening

label home:
    "The kettle clicks off."
    call shop
    jump evening

label park:
    "Leaves, and a board someone left out."
    call shop
    call minigame
    jump evening

label evening:
    menu:
        "Sleep" if energy > 1:
            jump end
        "One more chapter":
            "The lamp stays on."
            jump night

label shop:
    "What can I get you?"
    menu:
        "Tea":
            "Wrapped and paid."
        "Nothing today":
            "Another time, then."
    return

label minigame:
    "A shared game. Win or lose, the board folds away."
    jump minigame_done

label minigame_done:
    "Score noted."
    return

label night:
    call minigame
    call shop
    jump end

label secret:
    "A door that only opens when karma is high."
    jump end

label end:
    "The day is over."
    return

label peek:
    "This line is only a hover. It should stay off the map."
    return

screen town():
    imagemap:
        ground "town.jpg"
        hotspot (40, 80, 180, 120) action Jump("park")
        hotspot (240, 80, 180, 120) hovered Jump("peek") action Jump("home")
    textbutton "Shop" action If(karma > 5, Jump("shop_visit"), Jump("home"))
    textbutton "Play" action Function(renpy.call, "minigame")
    textbutton "Call it a night" action Call("evening")
    textbutton "Secret ending" action Jump("secret") if karma > 5
    if energy > 1:
        textbutton "Open late" action Jump("night")
