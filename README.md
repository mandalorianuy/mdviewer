# MDViewer (v0.1)

Visualizador de Markdown para macOS, rápido y liviano.

## Funciones v0.1

- Apertura de archivos `.md`.
- Render Markdown tipo WYSIWYG (lectura con formato).
- Selector de tipografía (familia) y tamaño.
- Exportación a PDF.
- App bundle macOS con declaración de tipos `.md` para asociación.

## Requisitos

- macOS 13+
- Xcode + toolchain con Swift 6.2

## Ejecutar en desarrollo

```bash
swift run
```

## Empaquetar `.app`

```bash
./scripts/package-app.sh
```

Se genera:

- `dist/MDViewer.app`

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
