# The actual radios still belong to the host.

The local bundle imports nmtui, impala, and bluetui when those tools are available on the host. `run nmtui` opens NetworkManager's detailed connection interface. `run impala` opens the iwd WLAN interface. `run bluetui` opens Bluetooth controls.

These tools manage real host hardware and services. Their app definitions deliberately use host mode, so Glyph asks you to type `allow host` for each launch. Agents and scripted commands can't approve it. Configuration still uses Glyph's copied HOME/XDG paths; connecting to a network or pairing a device changes the host's actual state. The sandbox cannot have its own physical Wi-Fi card by wishing very hard.

The tools depend on their host services: NetworkManager for nmtui, iwd for impala, and BlueZ for bluetui. Presence of the executable doesn't mean its daemon is configured. Glyph doesn't enable conflicting network services or change your connections during setup.

Ordinary apps remain confined. Network access is off by default; a `network = true` app shares host networking while retaining the restricted filesystem. The browser and explicitly installed agent CLIs need networking. Your links, files, and credentials don't become available merely because an app can reach the internet.

`guide rice` explains where tool configuration is copied. Use the launcher for installed apps and `store` to explore the wider TUI catalog. A hundred catalog entries is a good reason to add search, not a good reason to install all hundred.
