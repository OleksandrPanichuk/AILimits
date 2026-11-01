# Claude Limits

A small Windows tray app, similar to CodexBar on macOS, that shows your Claude Code plan limits:

- **Session (5h)** and **Weekly** usage, plus any per-model weekly limits, with how much is left
- When each window resets
- **Pace**: whether you are using the limit faster or slower than an even spread over the window, the
  projected usage at reset, and when you would run out at the current rate

The tray icon has two bars. The top bar is the 5-hour session and the bottom bar is the week.
Each bar's fill shows how much is **left**. Green means fine, amber means 75% or more is used or the
current pace runs out before reset, and red means the limit is reached or nearly reached.

Left-click the icon to open the popup. Right-click it for Refresh, Start with Windows, and Quit.

## How it gets the data

It reads the OAuth token that Claude Code saves when you log in
(`%USERPROFILE%\.claude\.credentials.json`, or `%CLAUDE_CONFIG_DIR%\.credentials.json`) and calls
`https://api.anthropic.com/api/oauth/usage`. This is the same data `/usage` shows in Claude Code.
The endpoint is undocumented, so a future Claude Code release may change it.

The app only reads the token. It never refreshes or rewrites it. If the token has expired, run any
`claude` command and Claude Code will refresh it. The app polls every 5 minutes and also refreshes
when you open the popup and the data is older than a minute.

On macOS (useful for development) it reads the `Claude Code-credentials` Keychain item instead.

## Pace math

For a window of length `W` that resets at `R`:

- elapsed = `W - (R - now)`
- even pace = `elapsed / W x 100%`. This is the white tick on each bar. A fill past the tick means you
  are ahead of pace.
- projected at reset = `used / elapsed x W`
- if the projection is over 100%, it runs out in `(100 - used) / (used / elapsed)`

There is no projection in the first 3% of a window, because early numbers are too noisy.

## Build

Requirements: Rust (stable), Node 18+, and on Windows the WebView2 runtime (already included in
Windows 10 and 11).

```powershell
npm install
npm run dev      # run with hot reload
npm run build    # installer in src-tauri\target\release\bundle\nsis\
```

`cargo test --manifest-path src-tauri/Cargo.toml` runs the unit tests for the parsing and pace logic.

The `Build Windows` GitHub Actions workflow builds the installer on `windows-latest` when you push a
`v*` tag or start it manually.
