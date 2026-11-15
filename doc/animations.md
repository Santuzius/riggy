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

Clip and avatar may rest in different poses (an A-pose clip on a T-pose avatar): riggy aligns each clip bone's rest direction with the avatar bone's, and its turn about that direction by a width across it (between the thighs, the shoulders or the knuckles), so every bone points where the clip's bone points and a palm faces where the clip's faces.

Actor and avatar also stand differently: the actor may slouch, keep the hands close to the body, or stand otherwise than an avatar's build and clothes allow. A bone map may name the pack's neutral clip, in which the actor stands still in their natural posture:

```toml
neutral = "idle_neutral"
```

riggy then calibrates every clip of that skeleton on it, for each avatar:

- **Trunk** (hips to head): the avatar keeps its own posture and moves only as far as the actor leaves theirs, so a bow or a slump still shows in full.
- **Arms**: the line from shoulder to wrist hangs out from vertical as riggy's idle holds the arms (`[idle] arm_angle`) where the actor's hangs as in the neutral clip; each arm swings out about the shoulder by the difference, fully while it hangs down and not at all once raised to horizontal, so the hands stay clear of wide hips or a coat.
- **Legs** stay with riggy's stance, which bends them from the clip's pelvis to where the feet stand.

riggy keeps these values in one file per pack and avatar, `<pack>/calibration/<avatar>.toml`, written from the neutral clip and rewritten when the clip, the bone map or `arm_angle` change:

```toml
auto = true                  # false: riggy reads the values as set by hand
neutral = "idle_neutral"
approved = ["idle_neutral"]  # clips checked on this avatar: listed in the tray, played by the behaviour

[actor]
arm_hang = [4.71, 4.52]      # degrees the actor's shoulder-to-wrist lines hang out, left and right

[actor.posture.chest]        # the actor's natural turn of each trunk bone against the clip's rest pose,
x = 0.31                     # degrees about X (positive leans forward), Y (turns left) and Z
y = -0.63
z = 0.03

[avatar]
arm_hang = 12.0              # degrees the avatar's arms hang out at ease
```

Without a calibration file or neutral clip, the trunk and arms point where the actor's do; for such a pack a file with `auto = false` can be written by hand. riggy's default Motifect map names `idle_neutral`. While `auto = true`, riggy overwrites edited values under `[actor]` and `[avatar]` with the measured ones and keeps `approved`. A file it cannot read is left as it is: riggy reports the error and uses its default until the file is fixed.

Where the user has no file yet, riggy starts from its default in `assets/animations/calibration/<pack>/<avatar>.toml`, which keeps the `approved` list when riggy measures the values. A clip missing from `approved` is not offered in the tray's Play animation menu and not played by the behaviour; Advanced settings → "Show not approved animations in behaviour" and "… in the list" lift that.

`[idle] base` in the behaviour file names a clip that loops whenever no other clip plays, in place of riggy's own idle motion (breathing, sway, head drift, arm pose); it blends into its own start over its last `fade` seconds, so the loop shows no seam. riggy's default is the Motifect pack's `idle_neutral`, used where it is approved on the avatar. Clips leave the eyes and the jaw to riggy's gaze and lip sync.

While a clip moves the torso, the head or the arms, riggy's own breathing, weight shift, head drift and arm pose give way to it there; the stance, blinking and lip sync stay.

[animation-checks.md](animation-checks.md) lists which clips have been checked on which avatar.

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
