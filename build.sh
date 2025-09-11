#!/bin/bash

# Build and install script for Typr
echo "🛠️  Building Typr..."

# Check if Deno is installed
if ! command -v deno &> /dev/null; then
    echo "❌ Deno is not installed. Please install Deno first:"
    echo "   curl -fsSL https://deno.land/install.sh | sh"
    exit 1
fi

# Compile the code
deno compile --allow-all --output dist/typr typr.ts

# Make script executable
chmod +x dist/typr

echo "✅ Build complete!"

# Try to install to user's local bin
if [ -d "$HOME/.local/bin" ]; then
    ln -sf "$(pwd)/dist/typr" "$HOME/.local/bin/typr"
    echo "📦 Installed to ~/.local/bin/typr"
    echo "💡 Make sure ~/.local/bin is in your PATH"
elif [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
    sudo ln -sf "$(pwd)/dist/typr" "/usr/local/bin/typr"
    echo "📦 Installed to /usr/local/bin/typr"
else
    echo "💡 Run './dist/typr' or add this directory to your PATH"
fi

echo ""
echo "🚀 Next steps:"
echo "  1. Add your OpenAI key to ~/.typr-settings.json"
echo "  2. typr shortcuts  # Setup keyboard shortcut"
echo "  3. Use your shortcut to record!"
