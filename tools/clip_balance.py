"""Balance of a figure from stick CSVs (`time` or `frame` column; from write_sticks or RIGGY_POSE_LOG): per sample, how far the body's centre lies ahead of the feet, how far the trunk leans and how the feet are pitched. Usage: clip_balance.py CSV [CSV ...]

Columns printed (means over the samples, then min..max):
  ahead  forward distance (z) of the body's centre (mean of the trunk, head and limb joints, fingers left out) ahead of the middle of ankles and toes, in % of the figure's height; a person standing quietly keeps it around 0 to +5 (centre over the middle of the feet, slightly before the ankles)
  lean   trunk angle from vertical in the side view, hips to head, degrees, positive leaning forward
  heel   ankle above toe height, in % of height: large positive values put the weight on the heels with the toes lifted relative to a flat foot (compare clip and avatar, the foot shapes differ)
"""
import csv, math, sys
from collections import defaultdict

BODY = ["hips", "spine", "chest", "upperChest", "neck", "head", "leftUpperArm", "rightUpperArm", "leftLowerArm", "rightLowerArm", "leftHand", "rightHand", "leftUpperLeg", "rightUpperLeg", "leftLowerLeg", "rightLowerLeg", "leftFoot", "rightFoot"]
FEET = ["leftFoot", "rightFoot", "leftToes", "rightToes"]

def samples(path):
    by = defaultdict(dict)
    for r in csv.DictReader(open(path)):
        key = r.get("time") or r.get("frame")
        by[float(key)][r["bone"]] = (float(r["x"]), float(r["y"]), float(r["z"]))
    return [by[k] for k in sorted(by)]

def measure(pose):
    height = pose["head"][1] - min(pose[f][1] for f in FEET if f in pose)
    body = [pose[b] for b in BODY if b in pose]
    cz = sum(p[2] for p in body) / len(body)
    feet = [pose[f] for f in FEET if f in pose]
    fz = sum(p[2] for p in feet) / len(feet)
    hips, head = pose["hips"], pose["head"]
    lean = math.degrees(math.atan2(head[2] - hips[2], head[1] - hips[1]))
    heel = sum(pose[s + "Foot"][1] - pose[s + "Toes"][1] for s in ["left", "right"]) / 2
    return 100 * (cz - fz) / height, lean, 100 * heel / height

for path in sys.argv[1:]:
    rows = [measure(p) for p in samples(path) if "head" in p and "leftToes" in p]
    if not rows:
        print(f"{path}: no full poses")
        continue
    cols = list(zip(*rows))
    stats = "  ".join(f"{name} {sum(c)/len(c):+6.1f} ({min(c):+.1f}..{max(c):+.1f})" for name, c in zip(["ahead %", "lean °", "heel %"], cols))
    print(f"{path.split('/')[-1]:40s} {stats}")
