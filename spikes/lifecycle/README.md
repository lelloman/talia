# Dashboard lifecycle fixture

A native Android Activity/JNI and capped browser Worker retain the same saved JS
fixture across background/hidden transitions. The host pauses guest pumping and
poll subscriptions, discards obsolete HTTP replies by epoch, then reads a fresh
snapshot and reconciles retained action IDs. HTTP work already dispatched to the
loopback server continues. Temporary edits survive retained runtimes; recreation
starts from the saved source. These are P0 adapters, not the P1 renderer or a
production networking stack. Subscription delivery is bounded snapshot polling.

Use the transport fixture server on a loopback port, then:

```
NODE_PATH=spikes/runtime/node_modules spikes/runtime/node_modules/.bin/esbuild spikes/lifecycle/web.js --bundle --format=esm --outfile=spikes/lifecycle/dist/web.js
node spikes/lifecycle/test-browser.mjs 18743
python3 spikes/lifecycle/test-android.py emulator-5556 18743
```

Build the Android APK with `spikes/runtime/build-android.sh` first. The Android
collector installs the APK and always force-stops/uninstalls it and removes ADB
reverse afterward. It uses actual Home/foreground transitions and process kill.
The browser collector starts its own Xvfb and Chromium, drives real tab switches
via CDP, and closes both. Playwright's forced focus emulation is unsuitable for
this visibility test. The fixture uses fixed test namespaces, never production
credentials or endpoints. Start a fresh fixture server for repeatable qualification.

Recorded `results` cover Chromium and the Android x86_64 emulator. Input hashes
identify this source revision; later revisions require fresh evidence. Physical
Android qualification remains TALIA-15. The earlier isolated runtime/transport
reports remain historical for source files changed by these additions.
