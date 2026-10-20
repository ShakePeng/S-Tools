# Kiri annotation subset

Source: https://github.com/yuxino/Kiri

- Fixed commit: `52d2d2f8fd613ce3341cbfae371009f06b123903`
- Upstream version: `1.6.25`
- License: MIT, Copyright (c) 2026 yuxino

This directory contains the framework-independent annotation, geometry,
history, text-layout, mosaic and rendering modules used by S-Tools. The React
editor is not copied; S-Tools provides its own Vue interface.

Local adaptations:

- Added the ellipse annotation type, hit testing, resizing and rendering.
- Added eight-direction line/arrow bounds scaling for the S-Tools toolbar.
- Kept all coordinates in display-local CSS points and let the caller provide
  source-image and export scale factors.

See the repository-level `THIRD_PARTY_NOTICES.md` for the complete license.
