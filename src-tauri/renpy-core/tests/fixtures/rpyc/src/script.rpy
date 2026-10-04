# Corpus compiled by Ren'Py 7 and 8. Comments are not stored in .rpyc.

define e = Character("E")
default score = 0

label start:
    "Hello."
    return

label room:
    e "Hi."
    menu room_menu:
        "Look":
            jump shop
        "Leave" if score > 0:
            return
    if score > 0:
        "yes"
    else:
        "no"
    call shop
    return

label shop:
    scene bg room with fade
    show eileen happy
    play music "a.ogg"
    pause
    while False:
        pass
    return

image bg room = "room.png"

transform slight:
    xalign 0.5
    linear 1.0 alpha 1.0

screen hello(name="Ada"):
    text "Hi [name]"
    textbutton "Go" action Return()

style mytext:
    size 20

init python:
    def greet():
        return 1

translate None strings:
    old "Hello."
    new "Hello."
