#!/bin/sh
# Builds Stet.app for iPhone and iPad from a compiled binary.
#
#   scripts/bundle-ios.sh <stet binary> <output folder> [version]
#
# The binary is one built for `aarch64-apple-ios` (a device) or
# `aarch64-apple-ios-sim` (the simulator); which one is read from the binary
# itself. A simulator bundle is ad-hoc signed, which is all a simulator asks
# for. A device bundle is signed only if told how:
#
#   STET_IOS_IDENTITY   a signing identity, as `security find-identity` names it
#   STET_IOS_PROFILE    the provisioning profile to embed
#   STET_IOS_BUNDLE_ID  the app's identifier, if the profile is for another
set -eu

binary="$1"
out="$2"
repo="$(cd "$(dirname "$0")/.." && pwd)"
version="${3:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$repo/Cargo.toml" | head -1)}"
bundle_id="${STET_IOS_BUNDLE_ID:-ai.archastro.stet}"
minimum="${IPHONEOS_DEPLOYMENT_TARGET:-16.0}"
app="$out/Stet.app"

if xcrun vtool -show-build "$binary" | grep -q SIMULATOR; then
    platform=iPhoneSimulator
    sdk=iphonesimulator
else
    platform=iPhoneOS
    sdk=iphoneos
fi

rm -rf "$app"
mkdir -p "$app"
cp "$binary" "$app/stet"
chmod 755 "$app/stet"
cp "$repo/LICENSE" "$app/LICENSE"

# The icon in the sizes the home screen, Settings and Spotlight ask for.
for size in 120 152 167 180; do
    sips -z "$size" "$size" "$repo/assets/icon.png" --out "$app/icon-$size.png" >/dev/null
done

cat > "$app/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Stet</string>
    <key>CFBundleDisplayName</key>
    <string>Stet</string>
    <key>CFBundleIdentifier</key>
    <string>${bundle_id}</string>
    <key>CFBundleExecutable</key>
    <string>stet</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleShortVersionString</key>
    <string>${version}</string>
    <key>CFBundleVersion</key>
    <string>${version}</string>
    <key>CFBundleSupportedPlatforms</key>
    <array>
        <string>${platform}</string>
    </array>
    <key>DTPlatformName</key>
    <string>${sdk}</string>
    <key>MinimumOSVersion</key>
    <string>${minimum}</string>
    <key>LSRequiresIPhoneOS</key>
    <true/>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.productivity</string>
    <key>NSHumanReadableCopyright</key>
    <string>© ArchAstro. MIT license.</string>
    <key>ITSAppUsesNonExemptEncryption</key>
    <false/>
    <!-- iPhone (the folding iPhone Duo with it) and iPad. -->
    <key>UIDeviceFamily</key>
    <array>
        <integer>1</integer>
        <integer>2</integer>
    </array>
    <!-- An empty launch screen is what says the app fills any screen it is
         given, at any size: each face of a folding phone, a split iPad. -->
    <key>UILaunchScreen</key>
    <dict/>
    <key>UIRequiredDeviceCapabilities</key>
    <array>
        <string>arm64</string>
        <string>metal</string>
    </array>
    <key>UISupportedInterfaceOrientations</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>UISupportedInterfaceOrientations~ipad</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationPortraitUpsideDown</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>UIApplicationSupportsIndirectInputEvents</key>
    <true/>
    <!-- The app's folder shows in Files, and documents open where they are. -->
    <key>UIFileSharingEnabled</key>
    <true/>
    <key>LSSupportsOpeningDocumentsInPlace</key>
    <true/>
    <key>CFBundleIcons</key>
    <dict>
        <key>CFBundlePrimaryIcon</key>
        <dict>
            <key>CFBundleIconFiles</key>
            <array>
                <string>icon-120</string>
                <string>icon-180</string>
            </array>
        </dict>
    </dict>
    <key>CFBundleIcons~ipad</key>
    <dict>
        <key>CFBundlePrimaryIcon</key>
        <dict>
            <key>CFBundleIconFiles</key>
            <array>
                <string>icon-152</string>
                <string>icon-167</string>
            </array>
        </dict>
    </dict>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Markdown document</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>LSHandlerRank</key>
            <string>Default</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>net.daringfireball.markdown</string>
            </array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Text document</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.plain-text</string>
            </array>
        </dict>
    </array>
    <key>UTImportedTypeDeclarations</key>
    <array>
        <dict>
            <key>UTTypeIdentifier</key>
            <string>net.daringfireball.markdown</string>
            <key>UTTypeDescription</key>
            <string>Markdown document</string>
            <key>UTTypeConformsTo</key>
            <array>
                <string>public.plain-text</string>
            </array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key>
                <array>
                    <string>md</string>
                    <string>markdown</string>
                    <string>mdown</string>
                </array>
            </dict>
        </dict>
    </array>
</dict>
</plist>
PLIST

if [ "$platform" = iPhoneSimulator ]; then
    codesign --force --sign - "$app" >/dev/null 2>&1
elif [ -n "${STET_IOS_IDENTITY:-}" ] && [ -n "${STET_IOS_PROFILE:-}" ]; then
    cp "$STET_IOS_PROFILE" "$app/embedded.mobileprovision"
    # The profile says what the app may do; it is signed to do exactly that.
    work="$(mktemp -d)"
    security cms -D -i "$STET_IOS_PROFILE" > "$work/profile.plist"
    /usr/libexec/PlistBuddy -x -c 'Print :Entitlements' "$work/profile.plist" > "$work/entitlements.plist"
    codesign --force --sign "$STET_IOS_IDENTITY" --entitlements "$work/entitlements.plist" "$app"
    rm -rf "$work"
else
    echo "Not signed: set STET_IOS_IDENTITY and STET_IOS_PROFILE to sign for a device." >&2
fi

echo "Built $app ($version, $platform)"
