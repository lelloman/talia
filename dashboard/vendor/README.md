# LelloDesign dependency

Talìa consumes Vue package **0.2.0**, built from upstream commit
`dd796de7d7364f11cb724500701f4701bf88f12f`. This was verified against Fucina main
on 2026-09-23. The snapshot includes the current fluid-workspace helpers, aligned
64 px headers, native controls and scoped typography beyond the initial release.

The archive is built without product-specific modifications. Export that commit
into a temporary directory, run `npm ci` then `npm pack` in `packages/vue`, and
copy the resulting archive to `lellodesign-vue-dd796de7.tgz`. `npm pack` runs the
upstream token generation, Vite build and TypeScript checks. Vue stays external.

No sibling checkout or private registry credential is needed to build Talìa.
To upgrade: export the selected upstream commit to a temporary directory, build
and pack there, replace the named archive and dependency, regenerate the lockfile,
and rerun the consumer browser checks. Do not modify generated package contents.

Source: https://fucina.homelab/lelloman/lellodesign

Archive SHA-256: `f2d3e4c1caf436214a5d12c78756178e37b9b6dbade0cf418e06c19eed3591c9`.
