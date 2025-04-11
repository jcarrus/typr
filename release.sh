#!/bin/bash

# Exit on error
set -e

# Configuration
APP_NAME="typr"
VERSION=$(node -p "require('./package.json').version")
GITHUB_USERNAME=$(gh api user | jq -r '.login')
TAP_REPO="homebrew-$APP_NAME"

echo "Starting release process for $APP_NAME v$VERSION"

# Check if gh CLI is installed
if ! command -v gh &>/dev/null; then
    echo "GitHub CLI (gh) is not installed. Please install it first:"
    echo "brew install gh"
    exit 1
fi

# Check if user is logged in to GitHub
if ! gh auth status &>/dev/null; then
    echo "Please login to GitHub first:"
    echo "gh auth login"
    exit 1
fi

# Build the app
echo "Building the app..."
npm run tauri build

# Create release directory
RELEASE_DIR="release"
mkdir -p $RELEASE_DIR

# Package the app
echo "Packaging the app..."
tar -czf $RELEASE_DIR/$APP_NAME.tar.gz ./target/release/bundle/macos/$APP_NAME.app

# Calculate SHA-256
SHA256=$(shasum -a 256 $RELEASE_DIR/$APP_NAME.tar.gz | cut -d ' ' -f 1)

# Create GitHub release
echo "Creating GitHub release v$VERSION..."
gh release create "v$VERSION" \
    $RELEASE_DIR/$APP_NAME.tar.gz \
    --title "Release v$VERSION" \
    --notes "Release v$VERSION of $APP_NAME"

# Get the release URL
RELEASE_URL="https://github.com/$GITHUB_USERNAME/$APP_NAME/releases/download/v$VERSION/$APP_NAME.tar.gz"

# Create or update the tap repository
echo "Updating Homebrew tap..."

# Check if tap repository exists
if ! gh repo view "$GITHUB_USERNAME/$TAP_REPO" &>/dev/null; then
    echo "Creating tap repository..."
    gh repo create "$GITHUB_USERNAME/$TAP_REPO" --public --description "Homebrew tap for $APP_NAME" --license mit
fi

# Clone or update the tap repository
if [ ! -d "$TAP_REPO" ]; then
    echo "Cloning tap repository..."
    gh repo clone "$GITHUB_USERNAME/$TAP_REPO"
else
    echo "Updating tap repository..."
    cd "$TAP_REPO"
    git pull
    cd ..
fi

# Create or update the formula
mkdir -p "$TAP_REPO/Formula"
cat >"$TAP_REPO/Formula/$APP_NAME.rb" <<EOF
class Typr < Formula
  desc "Useful dictation app built with Tauri"
  homepage "https://github.com/$GITHUB_USERNAME/$APP_NAME"
  url "$RELEASE_URL"
  sha256 "$SHA256"
  version "$VERSION"
  
  def install
    prefix.install "$APP_NAME.app"
  end
end
EOF

# Update README if it doesn't exist
if [ ! -f "$TAP_REPO/README.md" ]; then
    cat >"$TAP_REPO/README.md" <<EOF
# $APP_NAME Homebrew Tap

This repository contains the Homebrew formula for $APP_NAME.

## Installation

\`\`\`bash
brew tap $GITHUB_USERNAME/$APP_NAME
brew install $APP_NAME
\`\`\`
EOF
fi

# Commit and push changes to the tap repository
cd "$TAP_REPO"
git add .
git commit -m "Update $APP_NAME to v$VERSION"
git push

echo "Release completed successfully!"
echo "Users can now install your app with:"
echo "brew tap $GITHUB_USERNAME/$APP_NAME"
echo "brew install $APP_NAME"
