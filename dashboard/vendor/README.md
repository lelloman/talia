# LelloDesign dependency

Talìa consumes the actual Vue package, including its header controls, from
LelloDesign commit `76d8f662e4df130f4c127615dd6f4450966e1536`.
The published npm 0.1.0 predates those controls. This local archive is built from
that clean commit with `npm ci && npm pack` in `packages/vue`, as supported by the
upstream adoption guide. Its package version remains upstream 0.1.0; the filename,
lockfile integrity and commit identify this unpublished snapshot unambiguously.

No sibling checkout or private registry credential is needed to build Talìa.
To upgrade: export the selected upstream commit to a temporary directory, build
and pack there, replace the named archive and dependency, regenerate the lockfile,
and rerun the consumer browser checks. Do not modify generated package contents.

Source: https://fucina.homelab/lelloman/lellodesign

Archive SHA-256: `b28865da39b9473ed781d8c1080dfeaa0dd5478011028b765b1d5e0a9b971b82`.
