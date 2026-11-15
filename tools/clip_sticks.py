"""Stick figures of a clip (grey) and the avatar posed by it (colour) from the CSVs that `write_sticks` writes: front, side and top view at five frames, both figures standing on their hips. Usage: clip_sticks.py STEM OUT.png [hand]"""
import csv, sys
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

def load(path):
    poses = {}
    for r in csv.DictReader(open(path)):
        poses.setdefault(int(r["frame"]), {})[r["bone"]] = (float(r["x"]), float(r["y"]), float(r["z"]), r["parent"])
    return poses

stem, out = sys.argv[1], sys.argv[2]
hand = len(sys.argv) > 3 and sys.argv[3]
clip, avatar = load(stem + ".clip.csv"), load(stem + ".avatar.csv")
frames = sorted(clip)
picks = [frames[int(i * (len(frames) - 1) / 4)] for i in range(5)]
fig, axes = plt.subplots(3, 5, figsize=(16, 13))
for col, f in enumerate(picks):
    for pose, grey in [(clip[f], True), (avatar[f], False)]:
        hx, hy, hz, _ = pose["hips"]
        # Scale the clip figure to the avatar's hip height, so both overlap.
        k = avatar[f]["hips"][1] / hy if grey else 1.0
        for row, (h, v) in enumerate([(0, 1), (2, 1), (0, 2)]):
            ax = axes[row][col]
            for bone, (x, y, z, parent) in pose.items():
                if hand and not (bone.startswith(hand) and bone != hand + "UpperArm" and bone != hand + "Shoulder"):
                    continue
                if parent in pose:
                    p = pose[parent]
                    c = "0.6" if grey else "tab:red" if bone.startswith("left") else "tab:blue" if bone.startswith("right") else "black"
                    a = [(p[0] - hx) * k, p[1] * k, (p[2] - hz) * k]
                    b = [(x - hx) * k, y * k, (z - hz) * k]
                    ax.plot([a[h], b[h]], [a[v], b[v]], color=c, lw=4 if grey else 1.5)
    for row, label in enumerate(["front (x up y)", "side (z forward right, y up)", "top (x, z forward up)"]):
        ax = axes[row][col]
        ax.set_aspect("equal")
        if not hand:
            ax.set_xlim(-0.6, 0.6)
            ax.set_ylim((-0.5, 0.5) if row == 2 else (0, 1.9))
        ax.set_title(f"frame {f}, {label}", fontsize=9)
fig.suptitle(stem.split("/")[-1] + "  (grey: clip; red: left, blue: right: avatar)")
fig.tight_layout()
fig.savefig(out, dpi=70)
