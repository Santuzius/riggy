// Option A, hybrid: body on the clean T-pose fixture rig `humanoid`, fingers from `humanoid2` (both MIT), all with Unity's default limits (from argv[4], a default-limit Avatar) and twists. Each finger's first bone is carried from humanoid2's hand frame into humanoid's by the two hands' rest world rotations. writes the merged rig to argv[2].
import { readFileSync, writeFileSync } from "node:fs";
import { load, type Animator, type Avatar, type GameObject } from "unity-asset-reader";
import { humanRotations } from "../src/humanoid.js";
import { muscleIndex } from "../src/muscle.js";
import { fixtureEnv } from "./helpers.js";

type Q = [number, number, number, number];
const mul = (a: number[], b: number[]): Q => [
  a[3]! * b[0]! + a[0]! * b[3]! + a[1]! * b[2]! - a[2]! * b[1]!,
  a[3]! * b[1]! - a[0]! * b[2]! + a[1]! * b[3]! + a[2]! * b[0]!,
  a[3]! * b[2]! + a[0]! * b[1]! - a[1]! * b[0]! + a[2]! * b[3]!,
  a[3]! * b[3]! - a[0]! * b[0]! - a[1]! * b[1]! - a[2]! * b[2]!,
];
const inv = (q: number[]): Q => [-q[0]!, -q[1]!, -q[2]!, q[3]!];
const arr = (q: any): Q => (Array.isArray(q) ? (q as Q) : [q.x, q.y, q.z, q.w]);
const rotate = (q: number[], v: number[]) => { const p = mul(mul(q, [v[0]!, v[1]!, v[2]!, 0]), inv(q)); return [p[0], p[1], p[2]]; };

function fixture(name: string): Avatar {
  const env = fixtureEnv(`editor/6000.6.4f1/${name}/lz4/${name}`);
  for (const o of env.objects) {
    if (o.type !== 95) continue;
    const a = o.read<Animator>();
    const go = env.resolve(a.m_GameObject, o);
    if (go.status !== "found" || go.object.read<GameObject>().m_Name !== name) continue;
    const p = env.resolve(a.m_Avatar, o);
    if (p.status === "found") return p.object.read<Avatar>();
  }
  throw new Error(`no avatar ${name}`);
}
const body = fixture("humanoid"), hand = fixture("humanoid2");
const defaults = load(new Uint8Array(readFileSync(process.argv[3]!))).objects.find((o) => o.type === 90)!.read<Avatar>();
const d: any = defaults.m_Avatar.m_Human.data;
const limitOf = (hd: any, node: number) => hd.m_Skeleton.data.m_AxesArray[hd.m_Skeleton.data.m_Node[node].m_AxesId].m_Limit;
for (const av of [body, hand]) {
  const h: any = av.m_Avatar.m_Human.data;
  const copy = (to: number, from: number) => { if (to >= 0 && from >= 0) { const t = limitOf(h, to), f = limitOf(d, from); t.m_Min = { ...f.m_Min }; t.m_Max = { ...f.m_Max }; } };
  h.m_HumanBoneIndex.forEach((n: number, b: number) => copy(n, d.m_HumanBoneIndex[b]));
  for (const side of ["m_LeftHand", "m_RightHand"]) if (h[side]?.data) h[side].data.m_HandBoneIndex.forEach((n: number, i: number) => copy(n, d[side].data.m_HandBoneIndex[i]));
  h.m_ArmTwist = h.m_ForeArmTwist = h.m_UpperLegTwist = h.m_LegTwist = 0.5;
}
const hb: any = body.m_Avatar.m_Human.data, hh: any = hand.m_Avatar.m_Human.data;
const restWorld = (h: any) => {
  const nodes = h.m_Skeleton.data.m_Node, x = h.m_SkeletonPose.data.m_X;
  const w: Q[] = [];
  nodes.forEach((n: any, i: number) => { w[i] = n.m_ParentId < 0 ? arr(x[i].q) : mul(w[n.m_ParentId]!, arr(x[i].q)); });
  return w;
};
const wb = restWorld(hb), wh = restWorld(hh);

// The output skeleton: humanoid's nodes, then humanoid2's 15 finger nodes per hand under humanoid's hands.
const tos = (a: Avatar) => new Map(a.m_TOS);
const nameOf = (a: Avatar, h: any, i: number) => (tos(a).get(h.m_Skeleton.data.m_ID[i]) ?? "").split("/").pop();
const nodes: { name: string; parent: number; t: number[]; q: Q }[] = hb.m_Skeleton.data.m_Node.map((n: any, i: number) => ({ name: nameOf(body, hb, i), parent: n.m_ParentId, t: [hb.m_SkeletonPose.data.m_X[i].t.x, hb.m_SkeletonPose.data.m_X[i].t.y, hb.m_SkeletonPose.data.m_X[i].t.z], q: arr(hb.m_SkeletonPose.data.m_X[i].q) }));
const fingerMap: { out: number; src: number; first: boolean; carry: Q }[] = [];
const hands: number[][] = [];
for (const [s, side] of ["m_LeftHand", "m_RightHand"].entries()) {
  const bodyHand = hb.m_HumanBoneIndex[18 + s], srcHand = hh.m_HumanBoneIndex[18 + s];
  // Rotation taking humanoid2's hand frame to humanoid's, both at rest in the world.
  const carry = mul(inv(wb[bodyHand]!), wh[srcHand]!);
  const out: number[] = [];
  const srcOf = new Map<number, number>();
  hh[side].data.m_HandBoneIndex.forEach((src: number) => {
    const p = hh.m_Skeleton.data.m_Node[src].m_ParentId;
    const first = p === srcHand;
    const x = hh.m_SkeletonPose.data.m_X[src];
    const t = [x.t.x, x.t.y, x.t.z];
    srcOf.set(src, nodes.length);
    out.push(nodes.length);
    fingerMap.push({ out: nodes.length, src, first, carry });
    nodes.push({ name: nameOf(hand, hh, src)!, parent: first ? bodyHand : srcOf.get(p)!, t: first ? rotate(carry, t) : t, q: first ? mul(carry, arr(x.q)) : arr(x.q) });
  });
  hands.push(out);
}

// The merged rig for riggy: each node with its rest transform and, where muscles move it, its axes; a finger's first bone carried into humanoid's hand frame (its PreQ too, since its local rotation is PreQ·swing-twist·PostQ⁻¹).
const axesOf = (h: any, node: number) => { const id = h.m_Skeleton.data.m_Node[node].m_AxesId; if (id < 0) return null; const a = h.m_Skeleton.data.m_AxesArray[id]; return { pre: arr(a.m_PreQ), post: arr(a.m_PostQ), sgn: [a.m_Sgn.x, a.m_Sgn.y, a.m_Sgn.z], min: [a.m_Limit.m_Min.x, a.m_Limit.m_Min.y, a.m_Limit.m_Min.z], max: [a.m_Limit.m_Max.x, a.m_Limit.m_Max.y, a.m_Limit.m_Max.z] }; };
const rigNodes = nodes.map((n, i) => ({ ...n, axes: i < hb.m_Skeleton.data.m_Node.length ? axesOf(hb, i) : null }));
for (const m of fingerMap) { const a = axesOf(hh, m.src); if (a && m.first) a.pre = mul(m.carry, a.pre); rigNodes[m.out]!.axes = a; }
const round = (x: number) => Math.round(x * 1e7) / 1e7;
const r = (v: number[]) => v.map(round);
writeFileSync(process.argv[2]!, JSON.stringify({
  source: "unity-asset-reader test rigs humanoid (body) and humanoid2 (fingers), Unity 6000.6.4f1, MIT; Unity's default muscle limits; twists 0.5",
  human: hb.m_HumanBoneIndex,
  hands,
  root_x: r(arr(hb.m_RootX.q)),
  nodes: rigNodes.map((n) => ({ name: n.name, parent: n.parent, t: r(n.t), q: r(n.q), axes: n.axes && { pre: r(n.axes.pre), post: r(n.axes.post), sgn: n.axes.sgn, min: r(n.axes.min), max: r(n.axes.max) } })),
}, null, 1));
