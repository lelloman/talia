# LelloDesign dependency

Talìa consumes Vue package **0.3.0**, built from LelloDesign commit
`e95beaa352ff1ad8343f851d095b743b112d148b` in `../lellodesign` on 2026-09-24.
This adds the exported `LelloSegmentedControl`, with native radio behavior,
shared styling, documentation and a component playground. No registry release
or upstream push is required to consume this committed local archive.

The archive is built without product-specific modifications. Export that commit
into a temporary directory, run `npm ci` then `npm pack` in `packages/vue`, and
copy the resulting archive to `lellodesign-vue-e95beaa.tgz`. `npm pack` runs the
upstream token generation, Vite build and TypeScript checks. Vue stays external.

No sibling checkout or private registry credential is needed to build Talìa.
To upgrade: export the selected upstream commit to a temporary directory, build
and pack there, replace the named archive and dependency, regenerate the lockfile,
and rerun the consumer browser checks. Do not modify generated package contents.

Source: https://fucina.homelab/lelloman/lellodesign

Archive SHA-256: `891201da242256c785d11dd342363986f9cac5cf9e695e20a39ca25dc7e9ecf9`.
