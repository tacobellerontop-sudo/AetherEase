# AetherEase

A motion graphics editor for the desktop, written in Rust. AetherEase aims to
bring Alight Motion's approachable layer-and-keyframe workflow to Windows and
grow into a real alternative to After Effects.

![AetherEase editor](docs/screenshot.png)

| Home | New project |
|---|---|
| ![Home screen](docs/home.png) | ![New project dialog](docs/new-project.png) |

![A 3D scene with a camera orbiting around a null](docs/3d.png)

## What works today

- **Home screen**: one button to create a project and a grid of recent
  projects with live thumbnails. Right-click a project to remove it from the
  list.
- **New project dialog** like Alight Motion's: name, resolution (480p to 4K),
  aspect ratio (16:9, 9:16, 1:1, 4:3, 3:4, 4:5, 21:9), frame rate and
  background colour.
- **Autosave**: new projects are saved to your projects folder
  (`%APPDATA%\AetherEase\projects` on Windows) and every change is saved
  automatically a moment after you make it.
- **Editor layout** modelled on Alight Motion and adapted for desktop: a
  slim top bar (back to home, project name, undo/redo, a "more" menu), the
  canvas with a round **+** button for adding layers, a property panel on the
  right organised as icon pages (Move, Shape/Text/Image, Color, Border,
  Effects, Timing), and the timeline along the bottom with centred playback controls.
- **Layers**: rectangles (with rounded corners), ellipses, triangles,
  polygons, stars, text, and imported images (PNG, JPEG, WebP, BMP, GIF).
- **Color page**: solid fills or linear and radial gradients (with
  keyframable colours and angle), opacity, and 17 blending modes (Multiply,
  Screen, Overlay, Add, Difference, Hue, and more).
- **Effects page**: blur, drop shadow (colour, distance, angle, softness),
  glow (colour, radius, strength) and adjust color (brightness, contrast,
  saturation, hue), stacked in any order, each with
  keyframable settings that show on the layer's timeline bar.
- **One renderer for everything**: frames are composited on the CPU with
  tiny-skia, so the canvas, thumbnails and export match exactly. Text
  is drawn from glyph outlines, so it stays sharp under any transform.
- **Groups**: Ctrl+G (or the folder button) puts a layer in a group; the
  Move page's Group picker moves layers in and out, all without them moving
  on screen. Groups fold open in the timeline, move their contents in time,
  and have their own transform, opacity, blending and effects.
- **Masking**: set a layer's blending to **Mask** to show the layers below it
  in its group only where it is, or **Mask (inverted)** to cut it out.
- **Audio**: import MP3, WAV, OGG, FLAC or M4A from the + menu. Audio
  layers show their waveform on the timeline, play in sync with the
  playhead, can be moved and trimmed like any layer, and have a volume
  control.
- **Export** (top-right button or Ctrl+E): MP4 video with the audio mixed
  in, a looping GIF, or numbered PNG frames, at 100%, 75%, 50% or 25% size,
  rendered in the background with a progress bar. MP4 uses
  [ffmpeg](https://ffmpeg.org): install it, or put `ffmpeg.exe` next to
  `aetherease.exe`. GIF and PNG need nothing extra.
- **Null layers**: invisible layers (a dashed box in the editor, nothing in
  the output) that other layers can be parented to.
- **Parenting**: pick a Parent on the Move page and the layer follows that
  layer's position, scale and rotation. Parenting keeps the layer where it is.
- **Everything is 3D**: like After Effects with every 3D switch on, each
  layer has Depth (Z), Tilt X, Turn Y and Rotate Z, and starts flat on z = 0,
  where it looks exactly like 2D. Layers are drawn in perspective; layers on
  the same plane keep their stack order, and depth decides the rest, so
  nearer layers cover farther ones. Add a **Camera** layer to move, turn and
  zoom the view; parent it to a null to orbit. Without one, depth 0 looks
  exactly like 2D. See `examples/3d-demo.aether`.
- **Lights**: point, spot, parallel and ambient lights with colour and
  intensity (spots add cone and feather). As in After Effects, once a scene
  has a light, layers are shaded by it and go dark where no light reaches.
  Move a light by its sun marker; spot and parallel lights point along their
  dashed line and are aimed with Tilt X and Turn Y. See
  `examples/lights-demo.aether`.
- **Adjustment layers**: their effects change everything below them in the
  stack, inside their area (the whole frame by default). They stay flat over
  the frame whatever the camera does.
- **Solids**: a layer of colour the size of the canvas.
- **Pen and freehand drawing**: the tool strip at the canvas's top left has
  Select (V), Pen (P) and Brush (B). The pen places points with a click and
  curves with a drag; click the first point or press Enter to finish. The
  brush turns a freehand stroke into a smooth, editable path. Path layers
  show their points when selected: drag points and handles (Alt breaks the
  handle pair), double-click a point to make it smooth or sharp, Alt-click to
  delete it. Key the path to morph between shapes with the same number of
  points. Paths are filled and stroked (the Stroke page sets width and
  colour).
- **Video clips**: MP4, MOV, MKV, WebM, AVI and GIF, decoded through ffmpeg
  (the same one export uses). Clips are 3D layers like everything else, keep
  their sound (with a volume control) in playback and export, and are
  trimmed by dragging their bar's edges.
- **Canvas editing**: click to select, drag to move, corner handles to scale
  (Shift for uniform), top handle to rotate (Shift snaps to 15°). Scroll to
  zoom, middle or right drag to pan.
- **Keyframes** on position, scale, rotation, opacity, colour, size, corner
  radius, and border. Click a property's diamond to add a key; once a property
  has keys, editing it at another frame adds a key there automatically.
  Easing per key: linear, ease in, ease out, ease in & out, hold. Properties
  and easing are edited only in the sidebar.
- **Timeline**: one bar per layer, with its keyframes shown right on the bar.
  Click a key to select it and jump to it, drag it to retime, right-click for
  easing or delete. Scrub on the ruler, drag bars to move layers in time
  (keys move with them), drag bar edges to trim. Ctrl+scroll zooms.
- **Playback** at the project frame rate, with loop.
- **Project settings**: resolution presets (16:9, 9:16, 1:1, 4K...), frame
  rate, duration, background colour.
- **Undo/redo**, duplicate, reorder, show/hide, lock.
- **Save and open** projects as `.aether` files (JSON). Try
  `examples/demo.aether`.

## Build and run

Install Rust from <https://rustup.rs>, then:

```sh
cargo run --release                         # empty project
cargo run --release -- examples/demo.aether # open the demo
cargo run --release -- examples/3d-demo.aether
```

On Windows this produces a native `aetherease.exe` using DirectX 12 or Vulkan
through wgpu. CI builds a Windows release binary for every pull request.
On Linux, audio playback needs the ALSA headers to build
(`sudo apt install libasound2-dev`).

## Shortcuts

| Keys | Action |
|---|---|
| Space | Play / pause |
| ← / → | Previous / next frame |
| Home / End | First / last frame |
| Delete | Delete the selected keyframe, or the selected layer |
| Ctrl+G / Ctrl+Shift+G | Group / ungroup |
| Ctrl+D | Duplicate layer |
| Ctrl+Z, Ctrl+Y / Ctrl+Shift+Z | Undo, redo |
| Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S | New project, open, save, save as |
| Esc | Deselect |

## Code layout

| Path | What it holds |
|---|---|
| `src/model/anim.rs` | `Animated<T>` values, keyframes, easing |
| `src/model/groups.rs` | Grouping, ungrouping and moving layers between groups in place |
| `src/model/space.rs` | 3D transforms, parenting, the camera and perspective |
| `src/audio.rs` | Audio playback in step with the playhead, waveforms |
| `src/export.rs` | MP4 (via ffmpeg), GIF and PNG export on a background thread |
| `src/compose.rs` | The tiny-skia compositor: fills, gradients, images, blending, effects, adjustment layers |
| `src/light.rs` | Light layers shading the layers they reach |
| `src/model/vector.rs` | Editable vector paths: points, handles, morphing, freehand smoothing |
| `src/video.rs` | Video clips: probing and frame decoding through ffmpeg |
| `src/text.rs`, `src/path.rs` | Text layout and glyph outlines; vector path segments |
| `src/model/mod.rs` | `Project`, `Layer`, layer kinds, JSON save format |
| `src/render.rs` | Layer geometry, paint order, hit testing, editor guides |
| `src/history.rs` | Snapshot-based undo/redo |
| `src/app.rs` | Screens, editor state, file handling, autosave, shortcuts, playback |
| `src/recent.rs` | Recent projects list and the projects folder |
| `src/ui/` | Home screen and new project dialog, menu and toolbar, canvas viewport, inspector, timeline |

The UI is built with [egui](https://github.com/emilk/egui)/eframe. The canvas
preview, thumbnails and export all come from the same tiny-skia compositor.

## Roadmap

- Graph editor for custom bezier easing
- Lottie and Alight Motion project import
