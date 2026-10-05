// Writes src/lib/docs/renpy-reference.json.
// Summaries are original. Each entry links to the Ren'Py documentation,
// which is distributed with Ren'Py under the MIT licence:
// https://www.renpy.org/doc/html/license.html
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const base = 'https://www.renpy.org/doc/html/'
const doc = (page, hash = '') => `${base}${page}.html${hash}`

const entries = [
  stmt('label', 'label name:', 'Names a place the script can jump or call.', doc('label'), [{ name: 'name', doc: 'The label name. It must be unique in the game.' }]),
  stmt('menu', 'menu:', 'Asks the player to pick one of several choices.', doc('menus')),
  stmt('jump', 'jump label', 'Transfers control to a label and does not come back.', doc('label', '#jump-statement'), [{ name: 'label', doc: 'The label to continue from.' }]),
  stmt('call', 'call label', 'Runs a label and returns to the next line when that label returns.', doc('label', '#call-statement'), [{ name: 'label', doc: 'The label to run.' }]),
  stmt('return', 'return', 'Ends the current label and goes back to the caller, or to the main menu.', doc('label', '#return-statement')),
  stmt('if', 'if condition:', 'Runs the following block only when the condition is true. elif and else are further branches.', doc('conditional')),
  stmt('show', 'show image', 'Displays an image on a layer, on top of what is already there.', doc('displaying_images'), [{ name: 'image', doc: 'An image name, or a tag plus attributes.' }]),
  stmt('scene', 'scene image', 'Clears a layer and then shows an image on it.', doc('displaying_images'), [{ name: 'image', doc: 'The image to show after the layer is cleared.' }]),
  stmt('hide', 'hide image', 'Removes an image from the screen.', doc('displaying_images')),
  stmt('with', 'with transition', 'Shows the screen changes so far using a transition.', doc('transitions')),
  stmt('define', 'define name = value', 'Assigns a value once, when the game starts. Not saved, and not changed later.', doc('python', '#define-statement')),
  stmt('default', 'default name = value', 'Assigns a value the first time a new game starts. The value is saved.', doc('python', '#default-statement')),
  stmt('image', 'image name = displayable', 'Gives a name to an image the script can show.', doc('displaying_images', '#image-statement')),
  stmt('play', 'play channel file', 'Plays an audio file on a channel.', doc('audio', '#play-statement')),
  stmt('stop', 'stop channel', 'Stops the audio on a channel.', doc('audio', '#stop-statement')),
  stmt('queue', 'queue channel file', 'Plays a file after the current one on that channel finishes.', doc('audio', '#queue-statement')),
  stmt('voice', 'voice file', 'Plays a voice file for the next line of dialogue.', doc('audio', '#voice')),
  stmt('pause', 'pause', 'Waits for a click, or for a number of seconds when one is given.', doc('displaying_images', '#pause-statement')),
  stmt('screen', 'screen name():', 'Defines a screen of UI displayables and actions.', doc('screens')),
  stmt('transform', 'transform name:', 'Defines an ATL transform that can be used with at.', doc('transforms')),
  stmt('style', 'style name:', 'Defines or changes a style.', doc('style')),
  stmt('python', 'python:', 'Runs a block of Python. init python runs it at init time.', doc('python')),
  stmt('init', 'init:', 'Runs the block while the game is loading, before the game starts.', doc('python', '#init-statement')),
  stmt('translate', 'translate language:', 'Holds dialogue or strings rewritten for one language.', doc('translation')),
  fn('Character', 'Character(name, **properties)', 'Creates a character that can say dialogue.', doc('dialogue', '#Character'), [
    { name: 'name', doc: 'The name shown, or None for the narrator.' },
    { name: 'color', doc: 'Colour of the name.' },
    { name: 'who_color', doc: 'Same role as color.' },
    { name: 'what_color', doc: 'Colour of the dialogue.' },
    { name: 'image', doc: 'Image tag to show while this character speaks.' },
  ]),
  fn('DynamicCharacter', 'DynamicCharacter(name_expr)', 'A character whose name is read from an expression each time they speak.', doc('dialogue', '#DynamicCharacter'), [
    { name: 'name_expr', doc: 'A string giving the expression that produces the name.' },
  ]),
  fn('Dissolve', 'Dissolve(time)', 'A transition that fades from the old screen to the new one.', doc('transitions', '#Dissolve'), [{ name: 'time', doc: 'How long the fade takes, in seconds.' }]),
  fn('Fade', 'Fade(out_time, hold_time, in_time)', 'Fades to a colour, holds, then fades in the new screen.', doc('transitions', '#Fade'), [
    { name: 'out_time', doc: 'Seconds to fade out.' },
    { name: 'hold_time', doc: 'Seconds to hold the colour.' },
    { name: 'in_time', doc: 'Seconds to fade in.' },
  ]),
  fn('ImageDissolve', 'ImageDissolve(image, time)', 'Dissolves using the brightness of an image as the pattern.', doc('transitions', '#ImageDissolve'), [
    { name: 'image', doc: 'The control image.' },
    { name: 'time', doc: 'How long the dissolve takes.' },
  ]),
  transition('dissolve', 'A short fade. Defined by Ren\'Py, so it can be used by name.'),
  transition('fade', 'Fades through black.'),
  transition('pixellate', 'Dissolves through a pixelated image.'),
  transition('move', 'Slides images that changed position.'),
  transition('ease', 'Slides images, slowing at each end.'),
  fn('renpy.jump', 'renpy.jump(label)', 'Jumps to a label from Python.', doc('statement_equivalents', '#renpy.jump'), [{ name: 'label', doc: 'The label name.' }]),
  fn('renpy.call', 'renpy.call(label, *args, **kwargs)', 'Calls a label from Python and returns when it returns.', doc('statement_equivalents', '#renpy.call'), [{ name: 'label', doc: 'The label name.' }]),
  fn('renpy.show', 'renpy.show(name, at_list=None)', 'Shows an image from Python.', doc('statement_equivalents', '#renpy.show'), [{ name: 'name', doc: 'The image name.' }]),
  fn('renpy.hide', 'renpy.hide(name)', 'Hides an image from Python.', doc('statement_equivalents', '#renpy.hide'), [{ name: 'name', doc: 'The image name or tag.' }]),
  fn('renpy.scene', 'renpy.scene(layer="master")', 'Clears a layer from Python.', doc('statement_equivalents', '#renpy.scene'), [{ name: 'layer', doc: 'The layer to clear.' }]),
  fn('renpy.pause', 'renpy.pause(delay=None)', 'Waits for a click, or for delay seconds.', doc('statement_equivalents', '#renpy.pause'), [{ name: 'delay', doc: 'Seconds to wait. Omit it to wait for a click.' }]),
  fn('renpy.input', 'renpy.input(prompt, default="", length=None)', 'Asks the player to type a line of text.', doc('input'), [
    { name: 'prompt', doc: 'Text shown before the field.' },
    { name: 'default', doc: 'Text already in the field.' },
    { name: 'length', doc: 'Maximum number of characters.' },
  ]),
  fn('renpy.notify', 'renpy.notify(message)', 'Shows a short message over the game.', doc('other', '#renpy.notify'), [{ name: 'message', doc: 'The text to show.' }]),
  fn('renpy.say', 'renpy.say(who, what)', 'Displays a line of dialogue from Python.', doc('statement_equivalents', '#renpy.say'), [
    { name: 'who', doc: 'The character, or None.' },
    { name: 'what', doc: 'The dialogue.' },
  ]),
  fn('renpy.music.play', 'renpy.music.play(filenames, channel="music")', 'Plays audio on a channel from Python.', doc('audio', '#renpy.music.play'), [
    { name: 'filenames', doc: 'A file, or a list of files.' },
    { name: 'channel', doc: 'The audio channel.' },
  ]),
  fn('renpy.music.stop', 'renpy.music.stop(channel="music", fadeout=None)', 'Stops audio on a channel.', doc('audio', '#renpy.music.stop'), [
    { name: 'channel', doc: 'The audio channel.' },
    { name: 'fadeout', doc: 'Seconds to fade out.' },
  ]),
  fn('renpy.show_screen', 'renpy.show_screen(name, *args, **kwargs)', 'Shows a screen from Python.', doc('screens', '#renpy.show_screen'), [{ name: 'name', doc: 'The screen name.' }]),
  fn('renpy.call_screen', 'renpy.call_screen(name, *args, **kwargs)', 'Shows a screen and waits until it returns a value.', doc('screens', '#renpy.call_screen'), [{ name: 'name', doc: 'The screen name.' }]),
  fn('renpy.hide_screen', 'renpy.hide_screen(name)', 'Hides a screen.', doc('screens', '#renpy.hide_screen'), [{ name: 'name', doc: 'The screen name.' }]),
  action('Show', 'Show(screen, transition=None, *args, **kwargs)', 'Shows a screen. Used as a screen action.', doc('screen_actions', '#Show'), [{ name: 'screen', doc: 'The screen to show.' }]),
  action('Hide', 'Hide(screen, transition=None)', 'Hides a screen.', doc('screen_actions', '#Hide'), [{ name: 'screen', doc: 'The screen to hide.' }]),
  action('Jump', 'Jump(label)', 'Jumps to a label. Used as a screen action.', doc('screen_actions', '#Jump'), [{ name: 'label', doc: 'The label to jump to.' }]),
  action('Call', 'Call(label)', 'Calls a label. Used as a screen action.', doc('screen_actions', '#Call'), [{ name: 'label', doc: 'The label to call.' }]),
  action('ShowMenu', 'ShowMenu(screen)', 'Shows a main-menu or game-menu screen.', doc('screen_actions', '#ShowMenu'), [{ name: 'screen', doc: 'The menu screen.' }]),
  action('SetVariable', 'SetVariable(name, value)', 'Sets a store variable.', doc('screen_actions', '#SetVariable'), [
    { name: 'name', doc: 'The variable name.' },
    { name: 'value', doc: 'The value to assign.' },
  ]),
  action('SetScreenVariable', 'SetScreenVariable(name, value)', 'Sets a variable local to the current screen.', doc('screen_actions', '#SetScreenVariable'), [
    { name: 'name', doc: 'The screen variable.' },
    { name: 'value', doc: 'The value to assign.' },
  ]),
  action('ToggleScreen', 'ToggleScreen(screen)', 'Shows a screen, or hides it if it is already shown.', doc('screen_actions', '#ToggleScreen'), [{ name: 'screen', doc: 'The screen to toggle.' }]),
  action('Function', 'Function(callable, *args, **kwargs)', 'Calls a Python function. Used as a screen action.', doc('screen_actions', '#Function'), [{ name: 'callable', doc: 'The function to call.' }]),
  action('Return', 'Return(value=None)', 'Ends renpy.call_screen and returns a value.', doc('screen_actions', '#Return'), [{ name: 'value', doc: 'The value to return.' }]),
]

function stmt(name, signature, summary, url, params = []) {
  return { name, kind: 'statement', signature, summary, params, url }
}
function fn(name, signature, summary, url, params = []) {
  return { name, kind: 'function', signature, summary, params, url }
}
function transition(name, summary) {
  return { name, kind: 'transition', signature: name, summary, params: [], url: doc('transitions') }
}
function action(name, signature, summary, url, params = []) {
  return { name, kind: 'action', signature, summary, params, url }
}

const out = {
  license:
    "Summaries were written for Ren'Inspector. They link to the Ren'Py documentation, which ships with Ren'Py under the MIT licence (https://www.renpy.org/doc/html/license.html). The documentation pages themselves are not included.",
  entries,
}

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const dest = join(root, 'src', 'lib', 'docs', 'renpy-reference.json')
mkdirSync(dirname(dest), { recursive: true })
writeFileSync(dest, JSON.stringify(out, null, 2) + '\n')
console.log(`wrote ${entries.length} entries`)
