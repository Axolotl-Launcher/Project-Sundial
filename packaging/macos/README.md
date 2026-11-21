# macOS packaging

The Rust binary is `superlogical` from `platforms/macos`. A `.app` bundle needs
three things around it; this directory holds the ones that live in the repo.

## Layout a bundle must have

```
Superlogical.app/
  Contents/
    Info.plist
    MacOS/superlogical        <- cargo build --release -p superlogical_platform_macos
    Resources/AppIcon.icns
```

## Info.plist

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Superlogical</string>
  <key>CFBundleDisplayName</key><string>Superlogical</string>
  <key>CFBundleIdentifier</key><string>com.axolotl.superlogical</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>superlogical</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
```

## Building

```bash
cargo build --release -p superlogical_platform_macos
mkdir -p "Superlogical.app/Contents/MacOS" "Superlogical.app/Contents/Resources"
cp target/release/superlogical "Superlogical.app/Contents/MacOS/"
cp packaging/macos/Info.plist "Superlogical.app/Contents/"
# AppIcon.icns must be generated from a 1024x1024 PNG with `iconutil`.
```

## Signing and notarising

Unsigned bundles run only when macOS is told to allow them. Distribution needs:

```bash
codesign --force --deep --sign "Developer ID Application: <TEAM>" Superlogical.app
xcrun notarytool submit Superlogical.app --keychain-profile <profile> --wait
xcrun stapler staple Superlogical.app
```

Both steps need an Apple Developer account and credentials that must never be
committed. Wire them into CI as repository secrets when distribution starts.

## Not verified

The Rust code in `platforms/macos` has not been compiled on a Mac: the
development machine is Windows. The first macOS CI run is what will confirm it.
