# LelloDesign dependency

Talìa consumes the latest committed Vue source snapshot, **e0016d2**
(`e0016d27ce4ccd69bfea2cbec941597859505bf1`), packed on 2026-09-26.
Upstream still declares package version **0.3.1**; the open-workspace update is
unreleased, with 0.4.0 only a planned release. The archive filename includes the
commit so it cannot be confused with the earlier 0.3.1 artifact.

Source: https://fucina.homelab/lelloman/lellodesign

Build from a clean export of that commit, run `npm ci` and `npm pack` in
`packages/vue`, then copy the archive as `lellodesign-vue-e0016d2.tgz`.
Prepack runs token generation, Vite and TypeScript checks. Vue stays external.
No sibling checkout or private registry credential is needed to build Talìa.
Do not modify generated package contents. Update the dependency and lockfile and
rerun consumer browser checks on upgrade.

The web bundler emits Inter into `dist/fonts`; the deployment packager preserves
the font and OFL license, and the server serves WOFF2 with its correct MIME type.

Archive SHA-256: `962d74590ac5ad4ccf89a564a7f2f1f2d2f3f799a21dd525b975946f4374740c`.
