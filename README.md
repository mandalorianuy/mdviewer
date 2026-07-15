# MDViewer (v0.1)

Visualizador de Markdown para macOS, rápido y liviano.

La aplicación Swift actual se conserva, sin cambios semánticos, en
`legacy/macos-swift/` como baseline buildable de la migración multiplataforma.
La arquitectura y el inventario congelado están documentados en
[`docs/architecture/swift-baseline.md`](docs/architecture/swift-baseline.md).

## Funciones v0.1

- Apertura de archivos `.md`.
- Render Markdown tipo WYSIWYG (lectura con formato).
- Selector de tipografía (familia) y tamaño.
- Preferencia para abrir documentos en tabs o en ventanas separadas.
- Opción dentro de la app para asociar archivos `.md` con MDViewer.
- Exportación a PDF.
- App bundle macOS con declaración de tipos `.md` para asociación.
- Icono macOS propio (`AppIcon.icns`) generado automáticamente.
- Icono dedicado para documentos Markdown en Finder.

## Requisitos

- macOS 13+
- Xcode + toolchain con Swift 6.2

## Ejecutar en desarrollo

```bash
swift run --package-path legacy/macos-swift
```

Para verificar el baseline completo:

```bash
./scripts/verify-legacy-swift.sh
```

## Proyecto Xcode

Para generar el proyecto macOS listo para distribuir:

```bash
(cd legacy/macos-swift && xcodegen generate)
```

Se crea:

- `legacy/macos-swift/MDViewer.xcodeproj`

Build sin firma para validar el target:

```bash
xcodebuild -project legacy/macos-swift/MDViewer.xcodeproj -scheme MDViewer -configuration Release CODE_SIGNING_ALLOWED=NO build
```

## Empaquetar `.app`

```bash
./legacy/macos-swift/scripts/package-app.sh
```

Se genera:

- `legacy/macos-swift/dist/MDViewer.app`
- `legacy/macos-swift/macos/AppIcon.icns`
- `legacy/macos-swift/macos/MarkdownDocument.icns`

## Crear `.dmg`

```bash
./legacy/macos-swift/scripts/create-dmg.sh
```

Se genera:

- `legacy/macos-swift/dist/MDViewer-0.1.0.dmg`

El DMG incluye:

- `MDViewer.app`
- alias/symlink a `/Applications` para instalacion por drag and drop

## Notarizar `.dmg`

Para distribucion fuera de App Store necesitás un certificado `Developer ID Application`
instalado en el keychain y luego podés ejecutar:

```bash
CODESIGN_IDENTITY="Developer ID Application: Tu Nombre (TEAMID)" ./legacy/macos-swift/scripts/notarize-dmg.sh
```

## Instalar en macOS

```bash
./legacy/macos-swift/scripts/install-app.sh
```

Opcionalmente podés instalar en otro destino:

```bash
./legacy/macos-swift/scripts/install-app.sh "$HOME/Applications"
```

## Asociación de archivos `.md`

La app declara soporte de Markdown en su `Info.plist`.

- Si tenés `duti`, el instalador intentará configurar `.md`/`.markdown` por defecto.
- Sin `duti`, podés usar Finder: `Get Info` -> `Open with` -> `MDViewer` -> `Change All`.

## App Store

Archivos preparados para distribución:

- `legacy/macos-swift/project.yml`
- `legacy/macos-swift/macos/MDViewer.entitlements`
- `legacy/macos-swift/macos/ExportOptions-AppStore.plist`
- `legacy/macos-swift/scripts/archive-appstore.sh`
- `legacy/macos-swift/scripts/appstoreconnect_api.py`
