# Ren'Inspector

A desktop app for inspecting, editing and running [Ren'Py](https://www.renpy.org/) games, whether you are writing one or opening one that has already been released.

Ren'Inspector is an independent project. It is not made by, endorsed by, or affiliated with the Ren'Py project.

It reads `.rpy` scripts and compiled `.rpyc` files, shows the story as a map, and edits files in place. Changes to a released game go into a patch archive, so the game's own archives are left untouched. Running, live preview and builds use the game's engine or a Ren'Py SDK.

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

GPL-3.0-or-later. See [LICENSE](LICENSE).

The graph layout library, elkjs, is included under the Eclipse Public License 2.0 and is not covered by the GPL. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
