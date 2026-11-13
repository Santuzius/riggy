"""Rebuild the rest pose of a GLB whose bone nodes were flattened to identity transforms.

Some converted models (e.g. Fab's "fab-model-conversion" GLBs) keep the joint positions only in the skins' inverse bind matrices, while every bone node carries no transform. Such a model renders in its bind pose, but rotating a bone pivots around the model origin. This script sets each joint node's local transform so that its world matrix equals the bind pose, X · IBM⁻¹, where X is the world matrix of the skinned mesh node. Non-joint nodes keep their local transform.

Usage: python fix_bind_pose.py INPUT.glb OUTPUT.glb
"""
import json
import struct
import sys

import numpy as np


def rotation_matrix(q):
    x, y, z, w = q
    return np.array([
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ])


def quaternion(r):
    """Quaternion (x, y, z, w) of a proper rotation matrix."""
    trace = np.trace(r)
    if trace > 0:
        s = 2 * np.sqrt(trace + 1)
        q = [(r[2, 1] - r[1, 2]) / s, (r[0, 2] - r[2, 0]) / s, (r[1, 0] - r[0, 1]) / s, s / 4]
    else:
        i = int(np.argmax(np.diag(r)))
        j, k = (i + 1) % 3, (i + 2) % 3
        s = 2 * np.sqrt(1 + r[i, i] - r[j, j] - r[k, k])
        q = [0.0, 0.0, 0.0, (r[k, j] - r[j, k]) / s]
        q[i] = s / 4
        q[j] = (r[j, i] + r[i, j]) / s
        q[k] = (r[k, i] + r[i, k]) / s
    q = np.array(q)
    return q / np.linalg.norm(q)


def local_matrix(node):
    if 'matrix' in node:
        return np.array(node['matrix'], dtype=float).reshape(4, 4).T
    m = np.eye(4)
    m[:3, :3] = rotation_matrix(node.get('rotation', [0, 0, 0, 1])) @ np.diag(node.get('scale', [1, 1, 1]))
    m[:3, 3] = node.get('translation', [0, 0, 0])
    return m


def set_trs(node, m):
    """Write a similarity transform (rotation, uniform or axis scale, translation) as TRS."""
    scales = np.linalg.norm(m[:3, :3], axis=0)
    rotation = m[:3, :3] / scales
    if np.linalg.det(rotation) < 0:
        scales[0] = -scales[0]
        rotation[:, 0] = -rotation[:, 0]
    node.pop('matrix', None)
    node['translation'] = [float(v) for v in m[:3, 3]]
    node['rotation'] = [float(v) for v in quaternion(rotation)]
    node['scale'] = [float(v) for v in scales]


def read_glb(path):
    data = open(path, 'rb').read()
    json_length = struct.unpack('<I', data[12:16])[0]
    gltf = json.loads(data[20:20 + json_length])
    rest = data[20 + json_length:]
    binary_length = struct.unpack('<I', rest[0:4])[0]
    return gltf, rest[8:8 + binary_length]


def write_glb(path, gltf, binary):
    text = json.dumps(gltf, separators=(',', ':')).encode()
    text += b' ' * (-len(text) % 4)
    binary += b'\0' * (-len(binary) % 4)
    total = 12 + 8 + len(text) + 8 + len(binary)
    with open(path, 'wb') as f:
        f.write(struct.pack('<III', 0x46546C67, 2, total))
        f.write(struct.pack('<II', len(text), 0x4E4F534A) + text)
        f.write(struct.pack('<II', len(binary), 0x004E4942) + binary)


def inverse_bind_matrices(gltf, binary, skin):
    accessor = gltf['accessors'][skin['inverseBindMatrices']]
    view = gltf['bufferViews'][accessor['bufferView']]
    offset = view.get('byteOffset', 0) + accessor.get('byteOffset', 0)
    flat = np.frombuffer(binary, dtype='<f4', count=16 * accessor['count'], offset=offset)
    return flat.reshape(-1, 4, 4).transpose(0, 2, 1).astype(float)


def main(source, target):
    gltf, binary = read_glb(source)
    nodes = gltf['nodes']

    parents = {}
    for i, node in enumerate(nodes):
        for child in node.get('children', []):
            parents[child] = i

    worlds = {}

    def world(i):
        if i not in worlds:
            parent = world(parents[i]) if i in parents else np.eye(4)
            worlds[i] = parent @ local_matrix(nodes[i])
        return worlds[i]

    # Bind-pose world matrix of every joint; the first skin listing a joint wins (all skins agree in the models seen so far).
    bind_worlds = {}
    for i, node in enumerate(nodes):
        if 'skin' not in node:
            continue
        mesh_world = world(i)
        skin = gltf['skins'][node['skin']]
        for joint, ibm in zip(skin['joints'], inverse_bind_matrices(gltf, binary, skin)):
            bind = mesh_world @ np.linalg.inv(ibm)
            if joint in bind_worlds and not np.allclose(bind_worlds[joint], bind, atol=1e-3 * np.abs(bind).max()):
                print(f"warning: skins disagree on {nodes[joint].get('name')}", file=sys.stderr)
            bind_worlds.setdefault(joint, bind)

    # Walk top-down so each parent's new world matrix is known before its children.
    new_worlds = {}

    def visit(i, parent_world):
        if i in bind_worlds:
            set_trs(nodes[i], np.linalg.inv(parent_world) @ bind_worlds[i])
        new_worlds[i] = parent_world @ local_matrix(nodes[i])
        for child in nodes[i].get('children', []):
            visit(child, new_worlds[i])

    for scene in gltf['scenes']:
        for root in scene['nodes']:
            visit(root, np.eye(4))

    write_glb(target, gltf, binary)
    print(f'{len(bind_worlds)} joints rebuilt -> {target}')


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
