#!/bin/zsh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(awk -F'"' '/^version = / { print $2; exit }' src-tauri/Cargo.toml)"
APP_NAME="Islet"
APP="$ROOT/build/$APP_NAME.app"
# Fewer parallel jobs keep a laptop cooler; override with JOBS=8.
JOBS="${JOBS:-4}"

# Release settings from .env (see .env.example); the environment wins.
if [ -f "$ROOT/.env" ]; then
  while IFS='=' read -r key value; do
    case "$key" in ''|\#*) continue ;; esac
    if [ -n "$value" ] && [ -z "${(P)key:-}" ]; then export "$key=$value"; fi
  done < "$ROOT/.env"
fi
# Both are compiled in: the Microsoft sign-in client ID and the update source.
if [ -n "${MC_LAUNCHER_CLIENT_ID:-}" ]; then
  echo "==> Microsoft sign-in: built in"
else
  echo "==> warning: MC_LAUNCHER_CLIENT_ID is not set; Microsoft sign-in will ask for a client ID"
fi
if [ -n "${MC_LAUNCHER_UPDATE_URL:-}" ]; then
  case "$MC_LAUNCHER_UPDATE_URL" in
    https://*) echo "==> update source: $MC_LAUNCHER_UPDATE_URL" ;;
    *) echo "MC_LAUNCHER_UPDATE_URL must be an https address" >&2; exit 1 ;;
  esac
else
  echo "==> warning: MC_LAUNCHER_UPDATE_URL is not set; this build won't check for updates"
fi
export MC_LAUNCHER_CLIENT_ID="${MC_LAUNCHER_CLIENT_ID:-}" MC_LAUNCHER_UPDATE_URL="${MC_LAUNCHER_UPDATE_URL:-}"

# Keep this machine's paths (user name, cargo registry, checkout) out of the
# binary's panic messages. RUSTFLAGS splits on spaces, so a path with one is
# left as it is; each flag is added once, as a changed RUSTFLAGS rebuilds
# every dependency.
for map in "${CARGO_HOME:-$HOME/.cargo}=/cargo" "$ROOT=/islet"; do
  flag="--remap-path-prefix=$map"
  case "$map" in *" "*) continue ;; esac
  case " ${RUSTFLAGS:-} " in *" $flag "*) ;; *) RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }$flag" ;; esac
done
export RUSTFLAGS

echo "==> building web UI and release app (jobs: $JOBS)"
[ -d node_modules ] || npm install
npm test
CARGO_BUILD_JOBS="$JOBS" npm run tauri build -- --bundles app

echo "==> copying to $APP"
rm -rf "$APP"
mkdir -p "$ROOT/build"
cp -R "$ROOT/target/release/bundle/macos/$APP_NAME.app" "$APP"

# Strip only the final executable. Stripping proc-macro dylibs during the Cargo
# build breaks Rust 1.96, so Cargo's release profile keeps symbols.
strip -x "$APP/Contents/MacOS/islet" 2>/dev/null || true

# With a Developer ID certificate in the keychain, sign for real and send
# the app to Apple for notarization, so it opens on other Macs without the
# "unidentified developer" block. Set:
#   APPLE_SIGNING_IDENTITY  "Developer ID Application: Name (TEAMID)"
#   APPLE_ID, APPLE_TEAM_ID, APPLE_PASSWORD (an app-specific password)
# Without them the app is signed ad hoc, which is enough on this Mac only.
if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  echo "==> signing with $APPLE_SIGNING_IDENTITY"
  codesign --force --deep --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$APP"
  codesign --verify --strict --deep "$APP"
  if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ]; then
    echo "==> notarizing (a few minutes)"
    ZIP="$ROOT/build/$APP_NAME-notarize.zip"
    rm -f "$ZIP"
    ditto -c -k --keepParent "$APP" "$ZIP"
    xcrun notarytool submit "$ZIP" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_PASSWORD" --wait
    xcrun stapler staple "$APP"
    rm -f "$ZIP"
    spctl --assess --type execute --verbose "$APP"
  else
    echo "==> skipping notarization: set APPLE_ID, APPLE_TEAM_ID and APPLE_PASSWORD"
  fi
else
  echo "==> ad-hoc signing (set APPLE_SIGNING_IDENTITY to sign for distribution)"
  codesign --force --sign - --timestamp=none "$APP"
  codesign --verify --strict "$APP"
fi

# A disk image for sharing.
if [ "${DMG:-1}" = "1" ]; then
  DMG_PATH="$ROOT/build/Islet-$VERSION-$(uname -m).dmg"
  rm -f "$DMG_PATH"
  hdiutil create -volname "$APP_NAME" -srcfolder "$APP" -ov -format UDZO "$DMG_PATH" >/dev/null
  [ -n "${APPLE_SIGNING_IDENTITY:-}" ] && codesign --force --sign "$APPLE_SIGNING_IDENTITY" --timestamp "$DMG_PATH"
  echo "dmg: $DMG_PATH"
  # The DMG's entry in the update manifest, once its upload address is known.
  if [ -n "${RELEASE_BASE_URL:-}" ] && [ -n "${RELEASE_PAGE:-}" ]; then
    ARCH="$(uname -m)"; [ "$ARCH" = "arm64" ] && ARCH="aarch64"
    python3 "$ROOT/scripts/update-manifest.py" "$ROOT/build/latest.json" --version "$VERSION" \
      --page "$RELEASE_PAGE" --base-url "$RELEASE_BASE_URL" --notes "${RELEASE_NOTES:-}" "macos-$ARCH=$DMG_PATH"
    echo "upload $DMG_PATH to $RELEASE_BASE_URL, then build/latest.json to $MC_LAUNCHER_UPDATE_URL"
  fi
fi

echo
echo "built: $APP"
echo "version: $VERSION"
du -sh "$APP"
