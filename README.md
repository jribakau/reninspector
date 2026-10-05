# Ren'Inspector

A desktop app for inspecting, editing and running [Ren'Py](https://www.renpy.org/) games, whether you are writing one or opening one that has already been released.

Ren'Inspector is an independent project. It is not made by, endorsed by, or affiliated with the Ren'Py project.

It reads `.rpy` scripts and compiled `.rpyc` files, shows the story as a map, and edits files in place. Changes to a released game go into a patch archive, so the game's own archives are left untouched. Running, live preview and builds use the game's engine or a Ren'Py SDK.

## Download

Windows builds are attached to [releases](https://github.com/jribakau/reninspector/releases). The `.msi` installs the app. `reninspector-windows.zip` is the program by itself.

The files are not code-signed, so Windows SmartScreen may warn the first time. Choose "More info", then "Run anyway".

The welcome screen has **Open the demo**, a small game with a broken jump, an undefined image and an undefined speaker, so the project map and the Problems list have something to show without a Ren'Py SDK. It launches.

The demo is copied to the app's data folder the first time and is not overwritten afterwards, so your edits to it stay. Delete the `sample` folder there to get a fresh copy.

Help → **Copy diagnostic bundle** copies the app version, the system, and the open project's recent log lines. Nothing is sent anywhere. Your home folder is shown as `~`, but log lines can still contain other folder names, so read the text before posting it publicly.

Help → **Check for updates** asks before it installs a newer release, saves your open files, and then closes the app while the installer runs. It only finds published releases, not drafts or pre-releases. Updates install the `.msi` version, so a copy run from the zip is not updated in place.

## Editor shortcuts

The script editor is a CodeMirror buffer. A few of the bindings that are easy to miss:

| Keys | What it does |
|---|---|
| Alt+click | Add another caret |
| Alt+drag | Select a column |
| Ctrl+D | Add the next match of the selection |
| Alt+Up / Alt+Down | Move the current line |
| Ctrl+Shift+[ / ] | Fold or unfold the block at the caret (a label, menu, screen, or indented block) |
| Ctrl+/ | Toggle a comment |
| Shift+Alt+F | Format the script (tabs become spaces) |

Folding markers in the gutter can be turned off in Settings. Vim and Emacs keymaps are a setting too.

F12 goes to the definition under the caret, Shift+F12 finds every use, and F2 renames it. This works for labels, screens, images, transforms, characters, variables, functions and classes. Styles can be found but not renamed, because a style prefix derives other names.

Python blocks and `$` lines can use the [ty](https://docs.astral.sh/ty/) language server for completions, hover, signature help and diagnostics. It is off until you turn it on in Settings, which downloads a checked copy into the app's data folder. It does not read Ren'Py 7 projects, because those are Python 2.

## Develop

You need [Node.js](https://nodejs.org/), [Rust](https://www.rust-lang.org/) 1.77 or newer, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your system. Windows 10 and 11 already include the WebView2 runtime the app uses.

```sh
npm install
npm run tauri:dev
```

| Command | What it does |
|---|---|
| `npm run tauri:dev` | Run the app with hot reload |
| `npm run tauri:build` | Build an installer |
| `npm test` | Run the frontend tests |
| `npm run check` | Type-check the frontend |
| `cargo test --workspace` | Run the Rust tests, from `src-tauri` |
| `npm run notices` | Regenerate `THIRD_PARTY_NOTICES.md` after a dependency change |

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE). The one additional permission, for shipping elkjs, is in [LICENSE-EXCEPTION](LICENSE-EXCEPTION).

Hover text for Ren'Py statements and functions is written for this project and links to the [Ren'Py documentation](https://www.renpy.org/doc/html/). Most of Ren'Py, including that documentation, is under the MIT licence. The documentation pages themselves are not included.

The graph layout library, elkjs, is included under the Eclipse Public License 2.0 and is not covered by the GPL. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
