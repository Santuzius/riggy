# Animations

Humanoid avatars play animation clips from installed packs: a turn clip when they turn, talking gestures while riggy speaks, and now and then an idle clip in silence. Riggy's own idle motion, stance and lip sync stay on top. Without any pack installed, avatars move as before.

## Installing a pack

```sh
riggy install ./Animations/*.fbx
```

Each FBX file becomes one clip, a GLB of the skeleton and its motion. A file whose animation does not move (such as a pack's plain model) is refused. Clips are grouped by pack, named after the first folder above the file that is not called `animations`, `fbx` or similar:

```
~/.local/share/riggy/animations/
  <pack>/
    assets/<clip>.glb
    skeletons/<map>.toml
```

The tray menu opens this folder under Advanced settings → Animations folder.

## Bone maps

A clip's skeleton has its own node names. A bone map in the pack's `skeletons/` folder says which node is which humanoid bone, using the VRM 1.0 bone names:

```toml
[bones]
hips = "Hips"
spine = "Spine"
chest = "Spine1"
upperChest = "Spine2"
leftUpperLeg = "LeftUpLeg"
leftLowerLeg = "LeftLeg"
```

Riggy reads a clip through the first map in the folder whose nodes all exist in the clip. When installing, it adds its own map for the skeletons it knows (Motifect, and Mixamo-style names). For any other skeleton the install prints a hint, and a map written by hand into `skeletons/` takes effect at once. Bones a map leaves out stay still.

Clip and avatar may rest in different poses (an A-pose clip on a T-pose avatar): riggy aligns each clip bone's rest direction with the avatar bone's, so every bone points where the clip's bone points.

## Behaviour files

`~/.local/share/riggy/behaviour/behaviour.toml` sets which clips an avatar plays and how strongly; Advanced settings → Behaviour folder opens it. A file `<avatar>.toml` next to it changes single keys for one avatar, for example:

```toml
[talk]
intensity = 0.3
```

or `enabled = false` to switch the clips off for that avatar. Changes take effect within a second.

There are three sets, each played only where none of higher priority plays (turn before talk before idle):

|Set|Plays|Own keys|
|---|---|---|
|`[turn]`|while the avatar turns by more than `min_angle` degrees|`left`, `right`, `around` (from `around_angle` degrees on)|
|`[talk]`|while riggy speaks, one clip after another, each from a random place in it|`clips`|
|`[idle]`|in silence, one clip after a random pause|`clips`, `pause = [min, max]` in seconds|

Every set also has:

- `intensity`: how far the avatar follows the clip, from 0 (not at all) to 1 (fully).
- `fade`: seconds a clip takes to blend in and out; a clip that replaces another also fades the other out at least this fast.
- `bones`: `"upper"` (spine, head, arms, hands) or `"all"`. The legs and hips are best left to riggy's stance, which keeps the feet on the floor.

A clip is named by its file name without `.glb`, or as `pack/clip` where two packs have one of that name. Clips that are not installed are left out, and a set without any plays nothing.
