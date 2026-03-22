#!/usr/bin/env bash
# =============================================================================
# package-appimage.sh — Empaqueta reconstructor-gui como AppImage
#
# Requisitos:
#   - linuxdeploy (https://github.com/linuxdeploy/linuxdeploy)
#   - linuxdeploy-plugin-appimage
#   - ARCH=x86_64 (o arm64)
#
# Uso:
#   ./scripts/package-appimage.sh [VERSION]
#
# Ejemplo:
#   ./scripts/package-appimage.sh v1.0.0
# =============================================================================

set -euo pipefail

VERSION="${1:-$(git describe --tags --always)}"
ARCH="${ARCH:-x86_64}"
BINARY="target/release/reconstructor-gui"
APPDIR="AppDir"

if [[ ! -f "$BINARY" ]]; then
    echo "Error: $BINARY no encontrado. Ejecuta: cargo build --release --bin reconstructor-gui"
    exit 1
fi

# Verificar linuxdeploy
if ! command -v linuxdeploy &>/dev/null; then
    echo "Descargando linuxdeploy..."
    wget -q "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${ARCH}.AppImage"
    chmod +x "linuxdeploy-${ARCH}.AppImage"
    export PATH="$PWD:$PATH"
    LINUXDEPLOY="./linuxdeploy-${ARCH}.AppImage"
else
    LINUXDEPLOY="linuxdeploy"
fi

echo "Preparando AppDir..."
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"

# Copiar binario y assets
cp "$BINARY" "$APPDIR/usr/bin/reconstructor-gui"
cp model_manifest.toml "$APPDIR/usr/bin/"
cp -r config "$APPDIR/usr/bin/"

# .desktop file
cat > "$APPDIR/usr/share/applications/reconstructor.desktop" <<EOF
[Desktop Entry]
Name=ReconstructOR
Comment=Sistema OCR multimodelo para documentos
Exec=reconstructor-gui
Icon=reconstructor
Type=Application
Categories=Office;Graphics;
EOF

# Icono placeholder (reemplazar con PNG real)
if [[ -f "assets/icon-256.png" ]]; then
    cp assets/icon-256.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/reconstructor.png"
else
    # Crear un icono mínimo de 1x1 como placeholder
    printf '\x89PNG\r\n\x1a\n' > "$APPDIR/usr/share/icons/hicolor/256x256/apps/reconstructor.png"
fi

echo "Empaquetando AppImage..."
ARCH="$ARCH" "$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --output appimage \
    2>&1

# Renombrar con versión
APPIMAGE_OUT="ReconstructOR-${VERSION}-${ARCH}.AppImage"
mv ReconstructOR*.AppImage "$APPIMAGE_OUT" 2>/dev/null || true

echo ""
echo "✓ AppImage creado: $APPIMAGE_OUT"
sha256sum "$APPIMAGE_OUT"
