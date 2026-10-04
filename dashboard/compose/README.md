# Shared dashboard Compose host

An Android library that runs saved Talìa dashboards natively with LelloDesign.
Talìa Android includes it as `:dashboard-compose`.

- `QuickJs` loads `libtalia_dashboard_runtime.so` from [`../native`](../native).
  `build-runtime.sh` builds it in release mode for arm64-v8a, armeabi-v7a, x86 and
  x86_64; the Gradle build runs it automatically (NDK 27.0.12077973 and the four
  Rust Android targets are required). Contexts are thread-local, so every call runs
  on one dedicated thread.
- `DashboardSession` follows the web client: it validates the delivered package
  and resolves UI in the trusted context, runs the ViewModel in the guest context,
  and services guest requests against the package grants. Engine values stay
  TaliaValue-encoded until decoded inside the guest. The host supplies a
  `DashboardTransport`; credentials never enter QuickJS.
- `DashboardContent` renders a resolved contract v1 tree. Presentation roles match
  the web dashboard: card titles, status badges, metric type scale, card rows,
  meters and charts with area fill, threshold highlight and latest marker.

`node dashboard/compose/fixture.mjs` regenerates the Android device-test fixture
from the host dashboard catalog sources.
