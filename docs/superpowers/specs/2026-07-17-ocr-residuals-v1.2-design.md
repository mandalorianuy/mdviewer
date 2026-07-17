# OCR portable y captura universal v1.2 — diseño

Fecha: 2026-07-17

## 1. Resultado de producto

MDViewer v1.2 cierra los residuales conocidos de v1.1 sin reemplazar el núcleo
local y determinista. Reconoce texto dentro de imágenes embebidas en páginas PDF
que también contienen texto digital, mejora la calidad con evidencia medible y
ofrece un backend OCR productivo en macOS, Windows y Linux.

La captura universal continúa siendo una integración de plataforma que entrega
PDF u otro PDL al mismo conversor. El producto no necesita PyTorch, Docling, un
servicio remoto ni un segundo pipeline de Markdown.

La publicación inicial de v1.2 sigue siendo macOS 13+ Apple Silicon. El DMG debe
quedar firmado con Developer ID, notarizado, stapled y aceptado por Gatekeeper.
La publicación de binarios Windows/Linux queda detrás de sus gates de empaquetado
y validación, sin bloquear el backend OCR portable ni su CI.

La release y el tag `v1.1.0` son inmutables; todo cambio descrito aquí pertenece
a una versión posterior.

## 2. Alcance

Incluido:

- OCR de imágenes PNG/JPEG y páginas PDF escaneadas, conservando v1.1.
- OCR selectivo de imágenes embebidas en páginas PDF con texto digital.
- Proyección de las líneas reconocidas a la geometría de la imagen en la página.
- Conservación del asset visual original y supresión de texto OCR duplicado.
- Corpus determinista con métricas de recuperación de tokens, geometría,
  confianza, rotación y ausencia de pérdidas silenciosas.
- Apple Vision en macOS, `Windows.Media.Ocr` en Windows y Tesseract 5 en Linux.
- Contratos de empaquetado explícitos para los datos de idioma requeridos.
- Diseño y spikes verificables del adaptador universal de impresión para Windows
  y Linux.
- DMG macOS Apple Silicon firmado y notarizado para la nueva release.

Fuera de alcance:

- OCR generativo, reconstrucción visual pixel-perfect o corrección editorial
  automática del texto reconocido.
- Subir documentos o imágenes a servicios externos.
- PyTorch, Docling o descarga de modelos durante una conversión.
- Firmar/publicar paquetes Windows o Linux sin identidad, runners y evidencia de
  instalación específicos de esas plataformas.
- Sustituir la imagen original por el texto OCR.

## 3. Arquitectura OCR por plataforma

`mdconvert-ocr` mantiene un único contrato `OcrEngine`, sin APIs de UI ni de
impresión. Los conversores sólo conocen entradas codificadas, dimensiones,
fuente, líneas, confianza y rectángulos normalizados.

- macOS: Apple Vision, como en v1.1. No agrega modelos al bundle.
- Windows: `Windows.Media.Ocr.OcrEngine`, disponible desde Windows 10. El backend
  usa `RecognizeAsync(SoftwareBitmap)` y proyecta las líneas/palabras y sus
  rectángulos al contrato común.
- Linux: Tesseract 5 mediante API de biblioteca, no mediante un proceso shell.
  El paquete productivo fija la versión de `libtesseract`, Leptonica y los datos
  `eng`/`spa`; la conversión nunca descarga `traineddata`.

Las dependencias se declaran por `target_os`. Un fallo de inicialización o la
ausencia de datos lingüísticos produce `OcrError::Unavailable` o un diagnóstico
tipado y redactado, nunca un panic ni contenido del documento en logs.

Referencias normativas:

- Microsoft `Windows.Media.Ocr.OcrEngine`:
  <https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrengine>
- Tesseract OCR y su API:
  <https://github.com/tesseract-ocr/tesseract> y
  <https://tesseract-ocr.github.io/tessdoc/APIExample.html>

## 4. OCR de imágenes embebidas en PDF

PDFium continúa autenticando y extrayendo glifos, palabras e imágenes. El
algoritmo por página es:

1. Extraer siempre el texto digital y las imágenes.
2. Si la página no contiene texto digital, renderizarla y aplicar el OCR de
   página de v1.1; no aplicar además OCR individual a sus imágenes.
3. Si contiene texto digital, seleccionar únicamente imágenes elegibles y
   aplicar OCR a sus píxeles ya decodificados.
4. Proyectar cada rectángulo normalizado dentro de los límites de la imagen.
5. Rechazar una línea si su rectángulo se solapa materialmente con glifos
   digitales y el texto normalizado equivalente ya existe en esa región.
6. Insertar las demás líneas como palabras posicionadas y conservar la imagen
   como asset en el orden de lectura existente.

Una imagen es elegible cuando tiene al menos 64 px en cada dimensión, al menos
16 384 píxeles y ocupa al menos 1 % del área visible de la página. Estos límites
evitan OCR de iconos y arte decorativo pequeño. El presupuesto acumulado incluye
los píxeles de imágenes embebidas y mantiene los topes de 16 millones por
operación y 64 millones por documento. Cada imagen se procesa una sola vez.

El resultado registra páginas e imágenes intentadas, diferidas, sin texto y de
baja confianza. La indisponibilidad del OCR de una imagen no invalida el texto
digital ya extraído; produce una advertencia explícita.

## 5. Calidad y métricas

La calidad se valida sobre fixtures versionados, sin depender de una red ni de
un servicio mutable. El corpus contiene como mínimo:

- texto negro de alto contraste en inglés y español;
- dos tamaños de fuente y una línea rotada;
- imagen de bajo contraste e imagen invertida;
- PDF escaneado, PDF mixto por páginas y PDF con texto digital más una imagen
  textual embebida;
- icono/decoración que debe quedar fuera del OCR;
- solapamiento entre texto digital y una representación raster equivalente.

Métricas y gates:

- motores falsos: 100 % de recuperación de tokens esperados, texto exactamente
  una vez y geometría dentro del rectángulo fuente;
- fixture real estable de cada backend: al menos 95 % de recuperación de tokens
  normalizados y cero tokens esperados inventados por la aserción;
- ninguna línea válida se elimina sólo por baja confianza; se conserva y se
  advierte cuando la confianza es menor a 0,5;
- cero invocaciones para imágenes bajo umbral y cero OCR de imagen cuando ya se
  aplicó OCR a la página completa;
- resultados y warnings deterministas para el mismo motor y la misma entrada.

Las métricas son de regresión del producto, no una promesa universal de exactitud
para cualquier tipografía, idioma o degradación.

## 6. Captura universal por plataforma

El adaptador sólo captura el PDL y abre MDViewer con un archivo autenticado. La
conversión y la elección de destino permanecen en el producto principal.

- macOS: conservar el PDF Workflow instalado en el menú PDF de impresión.
- Windows: reemplazar el supuesto anterior de PAPPL por una Print Support App v4
  con impresora virtual. El componente MSIX/UWP declara
  `windows.printSupportVirtualPrinterWorkflow`, recibe PostScript/OXPS o PDF de
  passthrough y entrega el archivo a MDViewer. Referencia oficial:
  <https://learn.microsoft.com/en-us/windows-hardware/drivers/devapps/print-support-app-v4-design-guide>.
- Linux: Printer Application local sobre PAPPL/CUPS e IPP Everywhere, con cola
  explícita y entrega local a MDViewer.

Cada adaptador vive fuera del core OCR y requiere un smoke test en su sistema
real. La compilación cruzada no sustituye la prueba de instalación, aparición en
el diálogo de impresión, entrega del documento y desinstalación limpia.

## 7. Distribución macOS

El flujo vigente se reutiliza sin reducir garantías:

1. build reproducible `aarch64-apple-darwin` y auditoría de arquitecturas;
2. firma Developer ID de ejecutables, frameworks y bundle;
3. creación del DMG y envío al servicio notarial de Apple;
4. stapling en app/DMG, validación con `stapler` y aceptación con `spctl`;
5. receipt atómico con checksums, identidad, estado `signed`, `notarized` y
   `publishable`;
6. publicación sólo desde el commit y artefacto exactos verificados.

El perfil Keychain o las credenciales CI nunca se incorporan al repositorio.

## 8. Criterios de aceptación de v1.2

- Los casos de v1.1 continúan pasando sin cambios de salida incompatibles.
- Un PDF con texto digital e imagen textual produce ambos textos exactamente una
  vez, conserva la imagen y respeta geometría/presupuestos.
- Los tres backends productivos implementan el mismo contrato y pasan fixtures
  reales en runners nativos.
- CI compila y prueba core, CLI y desktop en macOS, Windows y Linux.
- Los adaptadores Windows/Linux poseen prototipo instalable y smoke evidence
  nativa antes de afirmar soporte universal publicado.
- El nuevo DMG macOS Apple Silicon está firmado, notarizado, stapled, aceptado
  por Gatekeeper y acompañado por checksum y receipt publicable.

