# Talìa icon motion

Open `index.html` or board **10 · Talìa icon motion** in `../../android.pen`.

Uses the unchanged paths and colors from `assets/brand/brand.svg`: two eye arcs and the central pupil. Adapted from LelloDesign's evolving product-logo motion reference, with smaller bounds appropriate to the eye. Balanced motion: arc displacement ≤2 x / 2.7 y units; pupil ≤3.5 x / 4.2 y; scale 0.94–1.06; arc rotation ±4°; opacity ≥0.82. Coordinates use the original 100 × 100 viewBox. Default display size is 48 px.

One continuous clock and smoothly interpolated deterministic weights blend movement, scale, rotation and opacity. Theme changes retain the clock. Pause, hidden/offscreen state and disposal stop drawing. Reduced motion immediately restores the original logo at full opacity. Completion stops immediately. Saved feedback uses the same eye with a single 240 ms settle.

Preview controls are for design review. Native Android integration is a separate step.
