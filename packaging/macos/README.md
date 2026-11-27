<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# macOS packaging

The Rust binary is `axlauncher` from `platforms/macos`. A `.app` bundle needs
three things around it; this directory holds the ones that live in the repo.

## Layout a bundle must have

```
Axolotl Launcher.app/
  Contents/
    Info.plist
    MacOS/axlauncher        <- node scripts/desktop.ts build
    Resources/AppIcon.icns
```

## Info.plist

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>AXLauncher</string>
  <key>CFBundleDisplayName</key><string>Axolotl Launcher</string>
  <key>CFBundleIdentifier</key><string>com.axolotl.axlauncher</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>axlauncher</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
```

## Building

```bash
node scripts/desktop.ts build
mkdir -p "Axolotl Launcher.app/Contents/MacOS" "Axolotl Launcher.app/Contents/Resources"
cp target/release/axlauncher "Axolotl Launcher.app/Contents/MacOS/"
cp packaging/macos/Info.plist "Axolotl Launcher.app/Contents/"
cp packaging/icons/axolotl-launcher.icns "Axolotl Launcher.app/Contents/Resources/AppIcon.icns"
```

## Signing and notarising

For distribution, set `AXLAUNCHER_MACOS_SIGN_IDENTITY` to the same Developer ID
Application identity you will use for the bundle before building. The build
signs all worker libraries before embedding them so the process can load them with
hardened runtime library validation enabled. Then sign and notarize the bundle:

```bash
codesign --force --options runtime --sign "$AXLAUNCHER_MACOS_SIGN_IDENTITY" "Axolotl Launcher.app"
xcrun notarytool submit "Axolotl Launcher.app" --keychain-profile <profile> --wait
xcrun stapler staple "Axolotl Launcher.app"
```

Both steps need an Apple Developer account and credentials that must never be
committed. Wire them into CI as repository secrets when distribution starts.

## Not verified

The Rust code in `platforms/macos` has not been compiled on a Mac: the
development machine is Windows. The first macOS CI run is what will confirm it.
