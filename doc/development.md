# Development tooling

## Running

```bash
nix develop -c cargo run                                     # uses riggy.toml
RIGGY_CONFIG=riggy.local.toml nix develop -c cargo run       # another config (riggy.local.toml is gitignored)
RIGGY_MODEL=path/to/model.glb nix develop -c cargo run       # another model with the configured mapping
```

riggy re-reads its config and the avatar's mapping file about once a second while it runs:

| File | Applied live | Needs a restart |
|---|---|---|
| `riggy.toml` | `[idle]`, `[blink]`, `[mouth]`, `[debug]` | `[avatar]`, `[window]`, `[speech]` |
| mapping (`assets/avatars/*.toml`) | `[rest]`, `[expressions.*]`, `[[props]]` | `scale`, `[humanoid]`, `[parts]` |

So poses, mouth shapes and props can be tuned in the editor while the window shows the result.

## Debug inputs

| Input | Effect |
|---|---|
| Mouse wheel | zoom between the full figure and the face |
| `1`–`5` | hold one mouth shape: `aa`, `ih`, `ou`, `ee`, `oh` |
| `0` | mouth back to rest |
| `F1` | print the model's node tree with avatar-space joint positions to the log |

- `RIGGY_DUMP_NODES=1` prints the same node tree once right after loading, without a key press. Positions are in model units (mm for the cyborg girl) in avatar space: +Y up, +Z the way the figure faces, +X its left. Use them to choose pivots and translation directions for the mapping file.
- `[debug] expressions = { blink = 0.0, aa = 1.0 }` in the config forces expression weights over everything blink and mouth code set; an empty table turns it off.
- `[window] zoom` sets the start zoom (0 = full figure, 1 = face).

## Screenshots

Screenshots never capture the real desktop. `agent-screenshot` (from portable-nixos, `services/agent/screenshot`) starts riggy on a private virtual X screen and saves exactly its window; the NVIDIA GPU renders there through Vulkan as on the desktop.

```bash
CMD="RIGGY_CONFIG=temp/my.toml nix develop -c target/debug/riggy" GRAB=root agent-screenshot temp/shot.png 'sleep 15'
```

- Build first (`nix develop -c cargo build`); `CMD` runs the binary without building.
- `GRAB=root` is needed: a direct window grab of the Vulkan window comes out black.
- `'sleep 15'` gives the model time to load and the camera time to frame it.
- Runs must be sequential; parallel runs collide on the virtual display.

`tools/expression_shots.sh OUTDIR [EXPRESSION ...]` builds on this: it shoots the face close-up at rest and with each expression at full weight (default: the five mouth shapes), with idle motion and blinking off, and puts the mouth regions side by side in `OUTDIR/all.png`. One shot takes about 20 s.

## Model preparation

- `tools/fix_bind_pose.py IN.glb OUT.glb` gives the Fab cyborg girl's bone nodes a rest pose (see the README).
- Models stay in the gitignored `assets/models/`; their licences do not allow redistribution.

## Mapping file features for models with gaps

- `[rest]` corrects the model's rest pose permanently, with deltas like an expression's. The cyborg girl's lips are slightly apart in the model; `[rest]` closes the jaw by 1.5°.
- `[[props]]` attaches simple shapes (sphere or cube, coloured, optionally unlit) to a bone. The cyborg girl has no mouth interior: two dark unlit ellipsoids (upper half on the head, lower on the jaw) form the cavity, two white ones the rows of teeth.
