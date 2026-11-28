# Englishmen 3D renderer

A software 3D renderer written in **Englishmen** (`.enm`), not in Rust.
Everything 3D — mesh generation, rotation, perspective projection, backface
culling, directional/flat shading, barycentric triangle rasterisation and
depth buffering — is implemented in Englishmen source.

There are two front ends:

| File | Output | Requirement |
| --- | --- | --- |
| `render_ascii.enm` | Shaded ASCII torus in the terminal | Any Englishmen install (`em render_ascii.enm`) |
| `render.enm` | Eight PNG frames + ASCII preview | The `em` built from this worktree (adds a `gfx` module and `math` trig) |

If you see `Unknown module 'gfx'`, you ran `render.enm` with a stock `em`. Use
`render_ascii.enm` instead, or build the worktree runtime below.

## Run the PNG version

From this directory, after building the runtime in the project root:

```bash
cd ..
cargo build
cd renderer
../target/debug/em render.enm
```

This writes `frame_1.png` … `frame_8.png` and prints an ASCII preview of the
last frame.

Options:

```bash
../target/debug/em render.enm --interpret   # tree-walking interpreter
../target/debug/em build                    # -> render.emb (uses englishmen.emc)
../target/debug/em render.emb               # run the compiled bytecode
```

It takes roughly 7–9 seconds for all eight 240×180 frames in a debug build.

## How it works

1. `render.enm` builds the torus surface procedurally with `math.sin` /
   `math.cos` — no mesh files.
2. Each quad is rotated about X and Y, moved into camera space and projected
   with `screen_x = W/2 + x*focal/z`.
3. Quads whose normals point away from the camera are culled; the rest are
   flat-shaded from a fixed light direction.
4. `raster()` walks the bounding box of each triangle, computes barycentric
   weights and writes a pixel only if it is nearer than the value in the
   z-buffer.
5. The `gfx` native module is only a pixel sink: `gfx.canvas(w, h)` gives an
   object with `set`, `get`, `clear`, `text` and `save(path)` methods. It does
   no geometry and no shading.

`render_ascii.enm` uses the same pipeline at 80×40 character resolution, but
implements its own `sine`/`cosine` with a Taylor series and writes to an array
of characters instead of calling `gfx`, so stock `em` can run it unchanged.

## Runtime changes in this project

This is a separate git worktree (`renderer-3d` branch), so the original
checkout is untouched. Two small, backwards-compatible additions were made to
the runtime so the renderer could be written in Englishmen:

- `src/modules/gfx.rs` — the framebuffer/PNG module described above, exposed
  as `import gfx`.
- `src/modules/math.rs` — added `sin`, `cos` and `tan` alongside the existing
  math functions (rotations need trigonometry).

Unit tests cover pixel set/get/save and the new trig functions.
