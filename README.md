# riggy

A desktop mascot for Linux written in Rust with [Bevy](https://bevy.org): a rigged 3D figure that idles (breathing, blinking, slight sway) and moves its mouth in sync with speech from a local text-to-speech server.

## Run

```bash
nix develop        # or `direnv allow` once
cargo run
```

The Rust toolchain comes from rustup (`rust-toolchain.toml`); the Nix shell adds Bevy's native libraries (Vulkan, Wayland, X11, ALSA, udev).

## Model

Models are not part of the repository. The default config expects `assets/models/cyborg_girl.glb`. The Fab download of this model stores its skeleton only in the skins' inverse bind matrices, so its bone nodes need a rest pose before they can be animated:

```bash
python3 tools/fix_bind_pose.py ~/Downloads/cyborg_girl/cyborg_girl.glb assets/models/cyborg_girl.glb
```

`python3` with NumPy is part of the Nix shell.

## Development

[doc/development.md](doc/development.md) covers live reloading, debug keys, node dumps and screenshots.
