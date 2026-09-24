# LelloDesign dependency

Talìa consumes Vue package **0.3.1**, built from LelloDesign commit
`5f793aa56c66855b3c19109fef825bed22cae0b2` in `../lellodesign` on 2026-09-24.
The changes adopt the approved
Material Design 3 segmented-button appearance and retain native radio behavior.
This is a local package archive; no registry release was made.

Build with `npm pack` in `../lellodesign/packages/vue`, then copy the resulting
archive to `lellodesign-vue-0.3.1.tgz`. The prepack step runs token generation,
Vite, and TypeScript checks. Vue stays external. The archive and npm lockfile
pin the exact consumed bytes; the upstream source is pinned to the commit above.

No sibling checkout or private registry credential is needed to build Talìa.
On upgrade, replace the archive and dependency, regenerate the lockfile, and
rerun consumer browser checks. Do not modify generated package contents.

Source: https://fucina.homelab/lelloman/lellodesign

Archive SHA-256: `4ee14394678c909fb8299c982cf4b44f9b3f1062118324a973c66632490df1bf`.
