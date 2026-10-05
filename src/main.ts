import './app.css'
import { mount } from 'svelte'
import App from './App.svelte'

// Chromium's menu. This listens on the way back up: CodeMirror refuses to run its own
// context-menu handlers once the event has already been cancelled on the way down.
window.addEventListener('contextmenu', (event) => event.preventDefault())

const app = mount(App, { target: document.getElementById('app')! })

export default app
