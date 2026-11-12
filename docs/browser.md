# Graphical browser alpha

`glyph-browser`, installed as `/apps/browser.app`, uses an existing Chromium/Chrome engine through its private remote-debugging pipes. HTML/CSS/JavaScript rendering stays in Chromium. PNG viewport frames stream through Kitty's graphics protocol; no HTML-to-text conversion is involved.

Keyboard and mouse events cross the pipe: text/paste, clicks, dragging, wheel scrolling, common special keys, forms, tabs, history, and reload. Downloads go to `/home/Downloads`; cookies/profile live under `/home/.local/share/glyph/browser`. Ctrl+L focuses the URL bar, Ctrl+T creates a tab, Ctrl+Tab switches, Ctrl+W closes, Alt+Left/Right navigates history, Ctrl+R reloads, Ctrl+Q exits.

The packaged definition grants networking and sets `GLYPH_BROWSER_ENGINE_SANDBOX=off` because nested Chromium sandbox setup is incompatible with this Bubblewrap layout. The helper refuses that setting outside Glyph, except its explicit disposable `--engine-test` diagnostic mode. Glyph's actual outer filesystem/process sandbox remains active, with no host home or desktop sockets. `/usr` and an installed `/opt/google/chrome` runtime are read-only.

An installed Chromium engine is required; `GLYPH_BROWSER_ENGINE` can select its executable. The browser is not bundled with a second browser engine. Rendering is capped by screenshot/RPC throughput (roughly two idle refreshes per second), so video/animation is not desktop-browser quality. Audio, camera/microphone, extension management, accessibility tree exposure, rich permission dialogs, and complete desktop download/tab UI are not implemented. Standard browser security updates come from the installed engine.

The diagnostic `--engine-test --interaction-test --profile DIR --output FILE.png` renders a real page, inserts text, clicks a button through the input bridge, verifies its state, and writes a screenshot. It uses an explicitly supplied disposable profile for validation.
