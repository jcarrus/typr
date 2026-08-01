#!/bin/bash
set -euo pipefail

# Build and install script for Typr
echo "🛠️  Building Typr..."

# Check if Deno is installed
if ! command -v deno &> /dev/null; then
    echo "❌ Deno is not installed. Please install Deno first:"
    echo "   curl -fsSL https://deno.land/install.sh | sh"
    exit 1
fi

for dependency in ffmpeg whisperkit-cli ollama; do
    if ! command -v "$dependency" &> /dev/null; then
        echo "❌ $dependency is not installed. Run: brew install ffmpeg whisperkit-cli ollama"
        exit 1
    fi
done

if ! command -v moonshine &> /dev/null; then
    echo "❌ Moonshine is not installed. Run: pipx install moonshine-voice"
    exit 1
fi

if [ "$(uname)" != "Darwin" ]; then
    echo "❌ The Typr menu-bar app currently supports macOS only."
    exit 1
fi

BUILD_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/typr-build.XXXXXX")"
APP="$BUILD_ROOT/Typr.app"
INSTALLED_APP="/Applications/Typr.app"
STAGED_APP="/Applications/.Typr.app.new.$$"
BACKUP_APP="/Applications/.Typr.app.backup.$$"

cleanup() {
    # Preserve the installed app if the build is interrupted during its
    # two-move replacement, then remove only paths this process owns.
    if [ ! -d "$INSTALLED_APP" ] && [ -d "$BACKUP_APP" ]; then
        mv "$BACKUP_APP" "$INSTALLED_APP"
    fi
    /usr/bin/python3 - "$BUILD_ROOT" "$STAGED_APP" "$BACKUP_APP" <<'PY'
import os
from pathlib import Path
import shutil
import sys

temporary_directory = Path(os.environ.get("TMPDIR", "/tmp")).resolve()
paths = [Path(raw) for raw in sys.argv[1:]]
safe_paths = [
    paths[0].parent.resolve() == temporary_directory
    and paths[0].name.startswith("typr-build."),
    paths[1].parent == Path("/Applications")
    and paths[1].name.startswith(".Typr.app.new."),
    paths[2].parent == Path("/Applications")
    and paths[2].name.startswith(".Typr.app.backup."),
]
for path, is_safe in zip(paths, safe_paths):
    if not is_safe:
        raise RuntimeError(f"Refusing to remove unexpected path: {path}")
    if path.is_symlink():
        path.unlink()
    elif path.exists():
        shutil.rmtree(path)
PY
}
trap cleanup EXIT

if [ -n "${TYPR_CODESIGN_IDENTITY:-}" ]; then
    CODESIGN_IDENTITY="$TYPR_CODESIGN_IDENTITY"
elif security find-identity -v -p codesigning | grep -q '"Typr Local Development"'; then
    CODESIGN_IDENTITY="Typr Local Development"
else
    echo "❌ No stable code-signing identity found. Ad-hoc signing resets macOS permissions."
    exit 1
fi
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp macos/Info.plist "$APP/Contents/Info.plist"
deno compile --allow-all --output "$APP/Contents/Resources/typr" typr.ts
swift build -c release --product Typr
cp .build/release/Typr "$APP/Contents/MacOS/Typr"
codesign --force --deep --sign "$CODESIGN_IDENTITY" "$APP"
codesign --verify --deep --strict "$APP"
ditto "$APP" "$STAGED_APP"
codesign --verify --deep --strict "$STAGED_APP"

pkill -x Typr 2>/dev/null || true
if [ -d "$INSTALLED_APP" ]; then
    mv "$INSTALLED_APP" "$BACKUP_APP"
fi
if ! mv "$STAGED_APP" "$INSTALLED_APP"; then
    if [ -d "$BACKUP_APP" ]; then
        mv "$BACKUP_APP" "$INSTALLED_APP"
    fi
    exit 1
fi
if [ -d "$BACKUP_APP" ]; then
    mv "$BACKUP_APP" "$BUILD_ROOT/Typr.app.backup"
fi

# Keep a conventional ignored build artifact without deleting the prior tree.
mkdir -p "$(pwd)/dist"
ditto "$INSTALLED_APP" "$(pwd)/dist/Typr.app"

mkdir -p "$HOME/.local/bin"
ln -sf "$INSTALLED_APP/Contents/Resources/typr" "$HOME/.local/bin/typr"

echo "✅ Installed Typr.app to /Applications"
echo "🚀 Run 'typr listen' to launch it"
