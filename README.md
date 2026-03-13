# MDViewer (v0.1)

Visualizador de Markdown para macOS, rápido y liviano.

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
swift run
```

## Proyecto Xcode

Para generar el proyecto macOS listo para distribuir:

```bash
xcodegen generate
```

Se crea:

- `MDViewer.xcodeproj`

Build sin firma para validar el target:

```bash
xcodebuild -project MDViewer.xcodeproj -scheme MDViewer -configuration Release CODE_SIGNING_ALLOWED=NO build
```

## Empaquetar `.app`

```bash
./scripts/package-app.sh
```

Se genera:

- `dist/MDViewer.app`
- `macos/AppIcon.icns`
- `macos/MarkdownDocument.icns`

## Instalar en macOS

```bash
./scripts/install-app.sh
```

Opcionalmente podés instalar en otro destino:

```bash
./scripts/install-app.sh "$HOME/Applications"
```

## Asociación de archivos `.md`

La app declara soporte de Markdown en su `Info.plist`.

- Si tenés `duti`, el instalador intentará configurar `.md`/`.markdown` por defecto.
- Sin `duti`, podés usar Finder: `Get Info` -> `Open with` -> `MDViewer` -> `Change All`.

## App Store

Archivos preparados para distribución:

- `project.yml`
- `macos/MDViewer.entitlements`
- `macos/ExportOptions-AppStore.plist`
- `scripts/archive-appstore.sh`
- `scripts/appstoreconnect_api.py`
