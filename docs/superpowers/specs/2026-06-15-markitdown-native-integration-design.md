# Integración nativa de funcionalidades "markitdown" en MDViewer

**Fecha:** 2026-06-15  
**Estado:** Aprobado para implementación  
**Autor:** Kimi Code (con validación del usuario)  

## 1. Objetivo

Extender MDViewer para que pueda abrir y visualizar archivos de múltiples formatos convertidos a Markdown, usando únicamente código nativo de Swift y APIs del sistema (sin depender de Python ni del paquete `markitdown`).

La implementación debe ser escalable: cada nuevo formato se agrega como un convertidor independiente, y el modelo de documento queda listo para futuras funciones de edición y guardado completo.

## 2. Contexto del proyecto

MDViewer v0.1.12 es una app macOS 13+ escrita en Swift con SwiftUI. Actualmente:

- Abre archivos `.md`, `.markdown`, `.mdown`, `.mkdn`, `.mkd`.
- Usa `DocumentGroup(viewing: MarkdownFileDocument.self)` para el manejo de documentos.
- Renderiza Markdown a HTML mediante `Down-gfm` y lo muestra en un `WKWebView`.
- Soporta búsqueda, temas, fuentes, exportación a PDF y asociación de archivos.
- No tiene tests.

## 3. Alcance

### 3.1. Incluido en este diseño (MVP)

Formatos nativos deterministas que se convertirán a Markdown:

- `.csv` → tabla Markdown.
- `.json` → lista anidada o bloque de código Markdown.
- `.xml` → representación jerárquica en Markdown.
- `.html` → Markdown simplificado (encabezados, párrafos, listas, enlaces, énfasis).
- `.zip` → contenedor; se explora su contenido y se convierte el primer archivo soportado, o se muestra un índice.

### 3.2. Fuera del MVP (fase 2)

Formatos que requieren parseo binario o APIs más complejas:

- `.pdf` (PDFKit).
- `.jpg`, `.png`, `.heic` (EXIF con ImageIO; OCR opcional con Vision).
- `.docx`, `.pptx`, `.xlsx` (OOXML manual).
- `.epub` (ZIP + HTML).
- URLs de YouTube (scraping/API de transcripción).

### 3.3. Excluido

- Transcripción de audio/voz (requiere `Speech` y no está en los requerimientos del usuario).

## 4. Arquitectura

```
Usuario abre un archivo
        │
        ▼
DocumentConversionService
        │
        ├── detecta tipo de archivo (extensión / UTType)
        │
        ├── selecciona DocumentConverter
        │
        ▼
MarkdownConversionResult
        │
        ▼
MarkdownFileDocument (rawMarkdown)
        │
        ▼
Render pipeline existente (Down-gfm → HTML → WKWebView)
```

### 4.1. Componentes nuevos

| Componente | Responsabilidad |
|---|---|
| `DocumentConversionService` | Orquesta la conversión. Recibe una `URL`, detecta el formato, ejecuta el convertidor adecuado y devuelve un resultado. |
| `DocumentConverter` (protocol) | Interfaz común: `canConvert(_:) -> Bool`, `convert(_:) throws -> MarkdownConversionResult`. El servicio se encarga de ejecutarlo off-main. |
| `MarkdownConversionResult` | Modelo inmutable con `markdown`, `sourceFormat`, `warnings` y `title`. |
| `FormatDetector` | Decide qué convertidor usar según extensión y, opcionalmente, sniffing de bytes/MIME. |
| Convertidores concretos | `CSVToMarkdownConverter`, `JSONToMarkdownConverter`, `XMLToMarkdownConverter`, `HTMLToMarkdownConverter`, `ZIPConverter`. |

### 4.2. Cambios en código existente

- `MarkdownFileDocument` se adapta para recibir una `URL` y decidir si requiere conversión antes de obtener el `rawMarkdown`. También acepta un `MarkdownConversionResult` para inicialización programática.
- `ContentView` muestra información de conversión y advertencias cuando corresponda.
- `Info.plist` registra los nuevos UTTypes para que Finder ofrezca "Abrir con MDViewer".
- `SettingsView` permite asociar formatos convertibles.

## 5. Flujo de datos

### 5.1. Archivo `.md` existente

No cambia. `DocumentGroup` → `MarkdownFileDocument` → render.

### 5.2. Archivo convertible (ej. `.csv`)

1. El usuario abre `invoice.csv` desde Finder o el botón "Abrir archivo".
2. `MarkdownFileDocument` detecta que la extensión no es Markdown.
3. Llama a `DocumentConversionService.convert(url:)`.
4. `FormatDetector` selecciona `CSVToMarkdownConverter`.
5. El convertidor genera la tabla Markdown.
6. Se devuelve `MarkdownConversionResult`.
7. `MarkdownFileDocument` usa `result.markdown` como `rawMarkdown`.
8. Se renderiza con el pipeline existente.
9. El usuario puede guardar el documento como `.md`. El archivo original nunca se sobrescribe; se comporta como un nuevo documento Markdown.

### 5.3. Concurrencia

- La conversión corre off-main, envuelta en `Task.detached` por el servicio.
- Los convertidores son síncronos internamente.
- Solo se lee de la URL de entrada; nunca se escribe en ella.

## 6. Modelo de datos

```swift
struct MarkdownConversionResult: Sendable {
    let markdown: String
    let sourceFormat: String
    let title: String?
    let warnings: [String]
}

protocol DocumentConverter: Sendable {
    var supportedExtensions: [String] { get }
    func canConvert(_ url: URL) -> Bool
    func convert(_ url: URL) throws -> MarkdownConversionResult
}

enum ConversionError: Error {
    case unsupportedFormat
    case fileNotReadable
    case conversionFailed(underlying: Error)
    case timeout
}
```

## 7. Manejo de errores y advertencias

### 7.1. Errores

| Error | Causa | Comportamiento UI |
|---|---|---|
| `unsupportedFormat` | No hay convertidor registrado. | Banner: "Formato no soportado todavía". |
| `fileNotReadable` | Sandbox deniega lectura. | Banner con opción a reintentar. |
| `conversionFailed` | Entrada corrupta o inválida. | Banner con mensaje técnico breve. |
| `timeout` | Conversión excede tiempo límite. | Banner: "La conversión tardó demasiado". |

### 7.2. Advertencias

Se almacenan en `MarkdownConversionResult.warnings` y se muestran en un panel colapsable:

- "La tabla HTML se convirtió a Markdown plano; estilos no se conservaron."
- "El ZIP contiene varios archivos; se convirtió el primero soportado."
- "Imagen sin metadatos ni texto detectable."

### 7.3. Logging

- Usar `os.log` con subsystem `com.facundo.mdviewer.conversion`.
- Debug: stack trace completo.
- Release: mensaje de error únicamente.

## 8. UX / UI

### 8.1. Apertura

- Doble clic en Finder para `.csv`, `.json`, `.xml`, `.html` y `.zip` abre MDViewer.
- El botón "Abrir .md" pasa a llamarse **"Abrir archivo"** y acepta las nuevas extensiones.
- Drag & drop funciona si el sistema lo permite.

### 8.2. Indicadores

Barra informativa debajo del toolbar cuando el documento proviene de conversión:

- Texto: "Convertido desde CSV".
- Advertencias con ícono de alerta.
- Botón "Guardar como Markdown".

### 8.3. Configuración

- En `SettingsView` se agrega la opción de asociar formatos convertibles con MDViewer.

### 8.4. Menú

- `File > Convertir a Markdown…` (opcional en MVP): genera un `.md` nuevo sin abrirlo.
- `File > Guardar como Markdown`: guarda el resultado de la conversión.

### 8.5. Accesibilidad

- Banners con `accessibilityLabel` descriptivo.
- Tablas generadas mantienen semántica HTML para VoiceOver.

## 9. Testing

Se agrega un target de tests al proyecto.

### 9.1. Tests unitarios por convertidor

- **CSV**: bien formado, comillas, saltos de línea, vacío.
- **JSON**: objeto, array, inválido.
- **XML**: simple, con atributos, malformado.
- **HTML**: encabezados, párrafos, listas, enlaces, complejo.
- **ZIP**: con un CSV, con varios archivos, corrupto.

### 9.2. Tests de integración

- `DocumentConversionService` elige el convertidor correcto.
- `MarkdownFileDocument` se inicializa desde `MarkdownConversionResult`.
- `ContentView` muestra advertencias cuando corresponde.

### 9.3. Fixtures

Archivos de ejemplo en `Tests/MDViewerTests/Fixtures/`.

## 10. Dependencias

- **Ninguna obligatoria para el MVP**.
- Opcional: `SwiftSoup` si se decide hacer conversión HTML de mejor calidad. Se evaluará durante la implementación.

## 11. Roadmap

### Fase 1 — MVP

- Arquitectura de conversión.
- Convertidores: CSV, JSON, XML, HTML, ZIP.
- Registro de UTTypes.
- Tests unitarios.
- UI de indicadores y guardado.

### Fase 2 — Formatos complejos

- PDF, imágenes (EXIF + OCR), Office, EPUB, YouTube.

### Fase 3 — Edición y exportación

- Editor Markdown con preview.
- Exportación a otros formatos.
- Guardado con metadatos de conversión.

## 12. Notas de decisión

- Se optó por **Swift nativo** en lugar de integrar el paquete Python `markitdown` para evitar dependencias externas y mantener la app autocontenida.
- Se eligió una **arquitectura extensible** desde el inicio para no reescribir el flujo de documentos cuando se agreguen más formatos.
- El modelo de documento se diseña como **editable/guardable** desde el MVP, anticipando la funcionalidad de edición futura.
