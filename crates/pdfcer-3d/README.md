# pdfcer-3d

3D models for PDF: PRC (ISO 14739-1) and glTF 2.0 / GLB reading, PRC
writing, a headless CPU rasterizer, draw data and GLSL ES 3.0 shaders for a
host's GPU, lights and cast shadows, and the camera (framing, projection,
picking). Pure Rust; the crate opens no files (external glTF buffers arrive
through a caller-supplied resolver).

Feature `render` (default) is the CPU rasterizer and turns on `textures`,
which decodes PNG and JPEG texture pictures. Reader-only consumers can build
with `default-features = false`.

Part of [pdfcer](https://github.com/KenM76/pdfcer), an MIT-licensed PDF engine
and command-line tool. The version follows the pdfcer workspace and the API is
not yet stable (0.x).

No GUI or network dependency; builds for wasm32.

Licence: MIT.
