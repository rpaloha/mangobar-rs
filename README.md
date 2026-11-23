# mangobar-rs

A status bar prototype for MangoWC, written in Rust with GTK4 and
gtk4-layer-shell. It displays tags, the current layout and window title, the
date and time, hidden windows, a system tray, network activity, CPU and RAM
usage, and volume.

This GTK4 build is heavier than the original MangoBar on the author's setup.
No comparative benchmark is available.

## Build and run

Install Rust and Cargo, GTK4, gtk4-layer-shell, and pkg-config. Run the bar
inside a MangoWC Wayland session with `mmsg` available on your PATH.

```sh
cargo build --release --locked
./target/release/mangobar-rs
```

The bar appears at the top of the screen as an overlay. It does not reserve
space for windows.

- The optional **APPS** button runs
  [fzf-ultra](https://github.com/rpaloha/fzf-ultra) from
  `~/.local/bin/fzf-ultra toggle`.
- Left-click a tag to switch to it. Right-click to toggle its visibility.
- Click the network indicator to switch between the network name and transfer
  rates. The network module uses `nmcli`.
- Click the volume indicator to mute or unmute. Scroll to adjust PipeWire volume
  by 2 percentage points. The bar displays 100% at 156% PipeWire volume.
  The module uses `wpctl` and `pactl subscribe`.
- The tray reads icons from an existing StatusNotifierWatcher and forwards
  Activate and ContextMenu requests. A built-in watcher and DBusMenu rendering
  are still pending.

Some icons require a font with Nerd Fonts glyphs.

## Contributing

Issues and pull requests are welcome. You can also fork the repository and
share your own version under the [MIT License](LICENSE).

## License

MIT. See [LICENSE](LICENSE).
