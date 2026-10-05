# APK 6 shell baseline

Package `com.lelloman.talia`, version 0.3.1 (6), embedded payload 17.
Contract: `bcbab3f2e8005d1eb097f41ad7e4744db6966b757f27ecfa4f4f312f95334301`.

Exported with `exportParavoidAndroidReleaseParavoidCompleteBaseline` and
`-PparavoidNewShell=true`, built against Paravoid `32461c8336` from JitPack.
This is a new shell generation, not a compatible update for APK 5: it carries the
updated installed Paravoid update engine. It keeps scheduled update checks and
downloads with a restart prompt, and trusts `com.lelloman.store` (release
certificate pin) as a local update trigger caller.
Do not regenerate this baseline from payload changes to bypass compatibility checks.
