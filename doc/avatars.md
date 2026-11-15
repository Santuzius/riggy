# Avatars

An avatar is a rigged 3D model in GLB format plus a mapping file that tells Riggy which bones to move and how its mouth shapes look, or a model in VRM 1.0 format, which brings both itself. It is a humanoid, which stands, breathes, blinks and moves its mouth, or a machine, which floats and spins its parts, and opens and glows while it speaks.

## Installing

```bash
riggy install ~/Downloads/robot.glb
```

copies the model to `~/.local/share/riggy/models/robot.glb`; the tray then lists it under "Avatar" as "robot". Some converted models (e.g. Fab's GLB conversions) keep their skeleton only in the skins' inverse bind matrices, while their bone nodes carry no transform; `riggy install` rebuilds the rest pose of such bones, so they can move.

A model whose file comes without its textures gets them as image files right after it:

```bash
riggy install ~/Downloads/robot.glb ~/Downloads/robot/ColorMap.jpg ~/Downloads/robot/ORM.png
```

puts the images into `~/.local/share/riggy/models/robot/`, where the mapping file's `[material]` names them. Clothing, a `.glb` without a skeleton, goes there the same way when it follows the avatar, and the mapping file's `[[clothing]]` names it.

## VRM avatars

A VRM 1.0 file (`.vrm`) is installed like a GLB file:

```bash
riggy install ./model.vrm
```

Riggy takes its humanoid bones and its expressions (blinking, mouth shapes, smile) from the file, and bevy_vrm1 moves its spring bones (hair, cloth); it needs no mapping file. A mapping file `avatars/NAME.toml` may still set `window_height`, and `[[cloth]]` for a garment without working spring bones; `riggy install` reads it from the user's mapping file or riggy's own and adds the chains to the installed copy, so a change takes effect when the model is installed again.

A VRM 0.x file is refused. Convert it once with the [VRM Add-on for Blender](https://github.com/saturday06/VRM-Addon-for-Blender): import it (File › Import › VRM), choose VRM 1.0 as the export version in the armature's VRM panel, and export it again (File › Export › VRM). The same works without opening Blender's window:

```python
# convert.py, run as: blender -b -P convert.py -- IN.vrm OUT.vrm
import sys, bpy, addon_utils
src, dst = sys.argv[sys.argv.index("--") + 1:][:2]
addon_utils.enable("bl_ext.user_default.vrm", default_set=True)
for o in list(bpy.data.objects): bpy.data.objects.remove(o)
bpy.ops.import_scene.vrm(filepath=src)
arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
arm.data.vrm_addon_extension.spec_version = "1.0"
bpy.ops.export_scene.vrm(filepath=dst, armature_object_name=arm.name)
```

## Mapping file

Riggy reads `~/.local/share/riggy/avatars/NAME.toml` for the model `NAME`. For the avatars it knows it copies its own mapping from `assets/avatars/` the first time; a new avatar needs one written by hand, with [cyborg_girl.toml](../assets/avatars/cyborg_girl.toml) as the example of a humanoid and [Ballytic813c_Reforged_ByLio3DW.toml](../assets/avatars/Ballytic813c_Reforged_ByLio3DW.toml) of a machine. The tray opens the current avatar's mapping file under "Advanced settings".

Positions and directions are in avatar space: +Y up, +Z the way the figure faces, +X its left.

| Key | What |
|---|---|
| `scale` | factor from model units to metres (0.001 for a model in millimetres) |
| `window_height` | share of the configured window height the avatar takes on the desktop (default 1), for a bulky avatar that should look smaller |
| `[humanoid]` | VRM humanoid bone name (`hips`, `head`, `jaw`, `leftUpperArm`, …) → node name in the model |
| `[expressions.NAME]` | node name → `{ rotate = [x, y, z], translate = [x, y, z] }`, the bone change at full weight; `rotate` is an axis-angle vector in degrees, `translate` an offset in model units |
| `[rest]` | bone changes applied permanently, e.g. lips that do not quite close |
| `[[clothing]]` | clothing without a skeleton the avatar wears: `file` (in the model's folder, where `riggy install` puts a skeleton-less `.glb` named right after the avatar), `scale` (factor from its units to metres, default 1), `offset` (shift in model units), `keep` (`[lowest, highest]` height in model units; parts outside are cut off), `gap` (least distance from the skin in model units; clothing closer or inside the body is pushed out); each of its points follows the bones of the nearest point of the body |
| `[[cloth]]` | VRM only, applied by `riggy install`: hangs spring chains in a garment that has none, so it swings like hair: `material` (start of the garment's material names), `bone` (humanoid bone the chains hang from, default `hips`), `top` (height where the garment starts to swing), `release` (humanoid bones whose binding of the garment moves to `bone`, e.g. the thighs, so its top does not follow them), `chains` (around the body, default 8), `uv` (texture rectangle `[u0, v0, u1, v1]` picking one part, such as a flap, which then swings on chains of its own across it; the entry without `uv` takes the rest), `segments` (per chain, default 3), `inset` (share of the garment's distance from the body's axis at which the chains hang, default 0.85), `stiffness`, `drag`, `gravity`, `hit_radius` (as VRM spring joints), `legs` (radii of the colliders on thighs and shins, 0 for none); [cyber_ver101.toml](../assets/avatars/cyber_ver101.toml) is the example |
| `[[cut]]` | VRM only, applied by `riggy install`: removes the triangles of the materials whose names start with `material` whose texture coordinates have their centre in `uv` (`[u0, v0, u1, v1]`), e.g. a painted lining that shows between two layers of a coat |
| `[[props]]` | simple shapes on a bone (`shape = "sphere"` or `"cube"`, `center`, `size`, `color`, `unlit`), e.g. a mouth interior or teeth the model lacks |
| `[parts]` | free part name → node name, for machines |
| `[machine]` | makes the avatar a machine and sets how it moves: `body` (the part that floats and wobbles), `facing` (turn of the body, axis-angle in degrees, e.g. `[90, 0, 0]` shows its top to the viewer), `hidden` (parts not shown), `float` (height in model units), `float_rate` (Hz), `wobble` (degrees), `spin` and `speaking_spin` (part → degrees per second, positive anticlockwise seen from above), `open_time` (seconds), `rest_glow` and `glow_gain` (weight of `glow` while silent, and per unit of the voice's level), `bloom` (halo around what glows brighter than white, 0 = none), `light` (`{ part, offset, color, lumens, range }`, a point light at the part, as bright as `glow`) |
| `[material]` | texture files in the model's folder for a model that has none: `base_color`, `orm` (occlusion, roughness, metalness in red, green, blue), `normal` (OpenGL convention), `emission`, and `emission_color` (linear RGB of the emission at full `glow`) |

With `hips` and the leg bones (`leftUpperLeg`, `leftLowerLeg`, `leftFoot`, the same on the right, `leftToes` and `rightToes` optional) mapped, the feet stay on the floor: the legs bend to follow the hips, the figure turns about the middle of its feet with the pelvis lagging towards them, and the feet step after a turn. The knees bend about the avatar's X axis.

Expressions Riggy drives on a humanoid: `blink`, `blinkLeft`, `blinkRight`, the mouth shapes `aa`, `ih`, `ou`, `ee`, `oh`, `cheeks` (raised with `ee` and `ih`) and `happy` (the idle smile). On a machine: `open`, the parts moved apart while it speaks, and `glow`, which sets the brightness of the `[material] emission` and follows the voice.

Node names match exactly, or after dropping the `_<digits>` suffix Sketchfab exports append. `RIGGY_DUMP_NODES=1 riggy` prints the model's node tree with joint positions, which helps to pick the bones. Changes to `[rest]`, `[expressions.*]` and `[[props]]` apply while Riggy runs, so mouth shapes can be tuned in the editor while the window shows the result; see [development.md](development.md) for the debug keys.
