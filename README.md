# fixmyborderpls

A tiny Windows 11 tray utility that sets window border colours and corner styles system-wide.

Sits silently in the system tray, applies your settings on startup, and catches new windows as they open.

---

## Features

- Set border colour to any hex or rgba value
- Toggle between square and rounded window corners
- Applies to all windows automatically, including File Explorer
- Config saved to `%APPDATA%\fixmyborderpls\config.json`
- Reload config from the tray without restarting
- Optional run on startup

## Installation

Download the latest `fixmyborderpls.exe` from [Releases](https://github.com/ilovealienz/fixmyborderpls/releases), drop it somewhere, and run it.

Or clone and build:

```powershell
git clone https://github.com/ilovealienz/fixmyborderpls
cd fixmyborderpls
cargo build --release
```

Then run `install.bat` to copy it to `%LOCALAPPDATA%\fixmyborderpls\` and add it to startup.

## Configuration

Edit `%APPDATA%\Roaming\fixmyborderpls\config.json`:

```json
{
  "color": "#000000",
  "square_corners": true,
  "run_on_startup": false
}
```

**color** accepts:
- `"#rrggbb"` — hex colour
- `"#rrggbbaa"` — hex with alpha
- `"rgba(r, g, b, a)"` — rgba with float or byte alpha

After saving, right-click the tray icon and click **Reload config**.

## Tray menu

| Option | Description |
|---|---|
| Reload config | Re-reads config.json and re-applies to all windows |
| Open config folder | Opens the config folder in Explorer |
| Quit | Exits |

## Anticheat

Safe to run alongside anticheats. Only uses `DwmSetWindowAttribute` — the same Windows API every modern app uses for its own border colour. No process injection, no hooks, no memory reading.
