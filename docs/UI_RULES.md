# Tangan UI rules

Keep the existing calm, minimal utility interface. This document describes
shared conventions; components and `src/styles.css` own exact dimensions,
colors, and spacing.

## Visual system

Use the established neutral surface, border, status, and typography tokens.
Light and dark themes use layered contrast without glaring cards. Use the mono
font for numeric values, paths, and dimensions. Avoid em dashes in UI copy.
Transparent imagery uses the shared transparency grid.

## Layout and controls

The launcher uses a compact responsive grid with up to three columns. Tool workspaces follow
files, settings, export location, then the primary action. Batch workspaces use
a file queue alongside a settings sidebar. Group settings in bordered cards,
keep one clear primary action, and keep secondary destructive controls muted
until interaction.

Reuse shared workspace, intake, progress, export, and completion components.
Image rows show thumbnails with an extension badge and icon fallback. PDF and
video rows use document/video icons; video metadata includes duration. Logo
previews remain uncropped. Keyboard focus and accessible control labels must
remain usable.

## Progress and completion

Sequential jobs show file position, overall percentage, a progress bar, and a
muted Cancel action. Completion summarizes outputs, sizes when relevant,
warnings, and failures. Use shared warning tokens and preserve tool-specific
OCR wording. All-failed results hide output-only actions. Canceled batches
explain that completed files remain and partial files were discarded.

Exports always auto-rename collisions and preserve source files. The app shares
one remembered export destination across tools.

## Palette editor

Keep the source preview and horizontal palette ribbon together in the main
studio card. Numbered pins map to swatches, with linked focus and sampling
feedback. Group extraction count separately from Add/Undo/Redo/Reset. The side
column contains the selected-color inspector and palette copy/export controls.
Manual changes and history operations keep the displayed palette count in
sync; automatic re-extraction establishes a new reset baseline.

Final visual smoke testing belongs to the owner. Automated checks establish
code and behavior coverage, not visual acceptance.
