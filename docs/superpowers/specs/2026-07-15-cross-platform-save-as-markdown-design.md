# MDViewer cross-platform con Guardar como Markdown universal

**Fecha:** 2026-07-15  
**Estado:** Diseño aprobado; plan de implementación preparado
**Producto:** MDViewer  
**Licencia:** MIT

## 1. Objetivo

Convertir MDViewer en un producto de escritorio completo y portable para ver,
editar y generar GitHub Flavored Markdown (GFM) en macOS, Windows y Linux. La
primera entrega pública será para macOS 13 o posterior en Apple Silicon e
incorporará una acción **Guardar como Markdown con MDViewer…** disponible desde
el menú PDF del diálogo de impresión de cualquier aplicación compatible con el
sistema de impresión de macOS.

La conversión será local, determinista y sin servicios remotos. La primera
versión no incluirá OCR ni modelos de machine learning. Un PDF impreso que no
contenga texto extraíble se rechazará con un diagnóstico claro y quedará
cubierto por la evolución de OCR prevista para v1.1.

## 2. Decisiones de producto

- MDPrinter no será una aplicación separada. La capacidad se consolida dentro
  de MDViewer.
- Se porta el producto completo, no sólo el conversor: visor, editor, preview,
  preferencias, convertidores, asociaciones de archivos e integraciones de
  impresión.
- La aplicación se migra de SwiftUI a Tauri 2 con un núcleo Rust compartido.
- La interfaz de escritorio será React con TypeScript y usará el WebView del
  sistema operativo mediante Tauri.
- El formato de salida de v1 es GFM.
- Las imágenes se guardan como `documento.md` más `documento.assets/` y se
  referencian con rutas relativas.
- La política de entrada es **source-first, PDF fallback**: se prefiere el
  formato original cuando MDViewer lo recibe directamente; el flujo universal
  de impresión usa el PDF generado por macOS.
- La primera distribución soporta únicamente `arm64-apple-darwin`. Intel queda
  fuera de v1, aunque la arquitectura no debe impedir agregarlo después.
- GitHub es el repositorio primario público. OneDev se conserva como mirror
  secundario.

## 3. Alcance de v1

### 3.1 Incluido

- Paridad funcional con el MDViewer Swift existente para abrir, visualizar,
  editar y guardar Markdown.
- Preview GFM y exportación existente que siga siendo necesaria para la
  paridad.
- Conversión directa de los formatos locales ya soportados por MDViewer,
  migrados al nuevo contrato común. La importación de URLs de YouTube queda
  fuera de v1 porque requiere red y no forma parte del flujo local acordado.
- Conversión HTML mediante un parser DOM, no mediante expresiones regulares.
- Conversión PDF de documentos digitales mediante PDFium y análisis geométrico.
- Extracción de imágenes, links y metadatos disponibles en el documento.
- Modelo intermedio común y emisor GFM único.
- Instalación, reparación y desinstalación de la acción universal de macOS.
- Diálogo Guardar como…, progreso, cancelación, advertencias y apertura del
  resultado en MDViewer.
- Empaquetado, firma Developer ID, notarización y verificación con Gatekeeper.
- CI del motor y de la aplicación en macOS, Windows y Linux. La publicación de
  binarios de v1 se limita a macOS Apple Silicon.

### 3.2 Excluido de v1

- OCR y conversión fiable de PDFs escaneados.
- PyTorch, Docling u otros runtimes de modelos de documento.
- Binarios para Intel, Windows o Linux.
- Una extensión de navegador que capture HTML directamente.
- Importación de YouTube u otras fuentes que requieran acceso de red.
- Sincronización cloud, cuentas, telemetría o procesamiento remoto.
- Promesas de reconstrucción perfecta de la semántica perdida al imprimir.

## 4. Arquitectura del repositorio

El repositorio `mdviewer` pasa a ser un monorepo con límites explícitos:

```text
apps/
  desktop/                 Aplicación Tauri 2 + React/TypeScript
crates/
  mdconvert-core/          Modelo intermedio, normalización y GFM
  mdconvert-pdf/           PDFium y reconstrucción geométrica
  mdconvert-html/          DOM HTML a modelo intermedio
  mdconvert-formats/       Convertidores restantes
  mdconvert-cli/           Interfaz headless y contrato de jobs
platform/
  macos/pdf-workflow/      Adaptador universal del menú PDF
legacy/
  macos-swift/             Implementación actual durante la migración
tests/
  fixtures/                Entradas compartidas
  golden/                  Salidas GFM y assets esperados
```

La aplicación Swift actual se mueve mecánicamente a `legacy/macos-swift/` y
debe continuar compilando mientras se construye la versión Tauri. No se elimina
hasta que la nueva aplicación alcance paridad funcional y pase los gates de
comparación. La eliminación ocurre en un commit dedicado después de crear un
tag que identifique la última versión Swift buildable.

## 5. Componentes

### 5.1 Aplicación desktop

La aplicación Tauri contiene:

- explorador y apertura de documentos;
- editor Markdown;
- preview GFM sanitizado;
- preferencias e integraciones;
- indicadores de conversión y advertencias;
- diálogos nativos de apertura y guardado;
- recepción de archivos y de URLs `mdviewer://`;
- asociación de extensiones y restauración de ventanas.

La UI no accede directamente a rutas arbitrarias. Todas las operaciones de
archivos y conversión pasan por comandos Tauri con capacidades mínimas y rutas
validadas por el núcleo Rust.

### 5.2 Núcleo de conversión

`mdconvert-core` no depende de APIs de macOS, Windows o Linux. Sus principales
tipos son:

```text
Document
  metadata
  blocks[]
  assets[]
  warnings[]

Block
  Heading | Paragraph | List | Table | Code | Quote | Image | ThematicBreak

Inline
  Text | Emphasis | Strong | Code | Link | LineBreak
```

Los extractores nunca escriben Markdown directamente. Generan este modelo,
el normalizador resuelve estructura común y un único emisor produce GFM. Este
límite permite cambiar un parser, incorporar OCR o agregar otro formato sin
modificar la UI ni la política de escritura.

### 5.3 Conversor PDF

`mdconvert-pdf` usa PDFium mediante bindings Rust mantenidos y un binario
PDFium fijado por versión y checksum. No usa PDFKit, de modo que el mismo motor
puede ejecutarse posteriormente en Windows y Linux.

El extractor obtiene:

- caracteres, palabras, fuentes, tamaños y bounding boxes;
- objetos de imagen y su posición;
- anotaciones de links;
- líneas, rectángulos y otros objetos útiles para tablas;
- dimensiones, rotación y metadatos de página.

El normalizador aplica reglas deterministas para:

- agrupar caracteres en palabras, líneas y párrafos;
- ordenar columnas y regiones de lectura;
- unir palabras partidas y líneas causadas por paginación;
- detectar títulos por tamaño relativo, peso, separación y contexto;
- reconocer listas por marcadores, numeración, sangría y continuidad;
- reconocer tablas por bordes explícitos o alineación repetida;
- retirar headers y footers repetidos entre páginas;
- colocar imágenes y links en el orden de lectura;
- emitir advertencias cuando una inferencia tenga confianza insuficiente.

La conversión no agrega encabezados artificiales `Página N` por defecto. El
número de páginas se conserva como metadata del resultado, no como contenido.

### 5.4 Conversor HTML

`mdconvert-html` usa un parser HTML5 con árbol DOM. El mapeo conserva headings,
párrafos, listas anidadas, tablas, blockquotes, bloques de código, énfasis,
links, imágenes y texto alternativo. Se ignoran `script`, `style`, contenido no
visible y atributos ejecutables. Las URLs se normalizan contra la URL base
cuando ésta existe.

El HTML recibido directamente ofrece mayor fidelidad que un PDF impreso. Una
extensión de navegador futura podrá usar esta misma entrada sin cambiar el
modelo ni el emisor.

### 5.5 CLI

`mdconvert-cli` expone el mismo motor para pruebas, automatizaciones y adapters:

```text
mdconvert convert <entrada> --output <archivo.md> --assets <directorio>
```

Los resultados programáticos se serializan como JSON versionado e incluyen
estado, metadata, advertencias y archivos generados. La CLI nunca habilita red
implícitamente.

## 6. Flujo universal de macOS

### 6.1 Instalación

La pantalla **Preferencias → Integraciones** muestra el estado de la acción y
ofrece **Instalar**, **Reparar** y **Desinstalar**. La instalación es por usuario
y no requiere privilegios administrativos.

MDViewer instala una herramienta firmada denominada
`Guardar como Markdown con MDViewer` en `~/Library/PDF Services/`. El instalador
verifica versión, firma y checksum. Después de una actualización, MDViewer
ofrece reparar automáticamente una herramienta desactualizada.

### 6.2 Recepción del trabajo

1. El usuario elige **Archivo → Imprimir → PDF → Guardar como Markdown con
   MDViewer…** en cualquier aplicación compatible.
2. macOS ejecuta la herramienta con el PDF temporal y las opciones CUPS.
3. La herramienta crea un UUID y copia atómicamente el PDF a
   `~/Library/Application Support/MDViewer/PrintJobs/<uuid>/input.pdf`.
4. El directorio y el archivo se crean con permisos exclusivos para el usuario.
5. La herramienta abre `mdviewer://print/<uuid>` y finaliza sólo después de
   confirmar que el job quedó persistido.
6. MDViewer valida que el identificador sea un UUID y que la ruta canónica se
   encuentre dentro de `PrintJobs`.
7. MDViewer se activa y muestra el diálogo **Guardar como…** con un nombre
   derivado del título del trabajo.

La URL nunca contiene una ruta de archivos ni datos del documento.

### 6.3 Conversión y salida

Después de elegir el destino, MDViewer muestra progreso y permite cancelar. El
motor trabaja en un directorio temporal dentro del mismo volumen del destino.
Al finalizar mueve el `.md` y su carpeta `.assets` al destino.

- Si no hay imágenes, no se crea `.assets`.
- Si el `.md` existe, el diálogo nativo solicita confirmación.
- Si la carpeta `.assets` ya existe, MDViewer sólo la reemplaza cuando posee un
  manifiesto válido creado previamente por MDViewer y el usuario confirma.
- Una carpeta desconocida nunca se elimina ni se mezcla silenciosamente.
- Cancelar o fallar elimina staging y no deja una salida parcial.
- El resultado exitoso se abre en MDViewer.

## 7. Política source-first

La fidelidad depende de conservar la fuente semántica cuando esté disponible:

- HTML abierto directamente usa DOM.
- DOCX, EPUB y otros formatos usan su extractor específico.
- PDF abierto directamente o recibido desde impresión usa el extractor PDF.
- La integración universal de impresión siempre recibe PDF porque ése es el
  contrato del PDF Workflow de macOS.

MDViewer no intentará obtener HTML de otra aplicación durante un trabajo de
impresión. Capturar HTML queda reservado para una integración explícita de
navegador posterior.

## 8. Formato de salida

- Dialecto: GitHub Flavored Markdown.
- Encoding: UTF-8.
- Saltos de línea: `LF` en todas las plataformas.
- Links: Markdown estándar con URL escapada.
- Tablas: sintaxis GFM; celdas multilinea se normalizan sin producir Markdown
  inválido.
- Imágenes: `![alt](documento.assets/imagen-001.png)`.
- Assets: nombres deterministas, sanitizados y sin segmentos de ruta externos.
- Metadata de conversión: se conserva en el modelo y en la UI. El archivo no
  agrega YAML frontmatter por defecto para mantener una salida GFM limpia. Una
  preferencia futura podrá habilitarlo sin cambiar el conversor.

## 9. Errores, advertencias y recuperación

### 9.1 Errores que detienen el trabajo

- entrada inexistente, ilegible, corrupta o cifrada sin credenciales;
- PDF sin texto extraíble en v1;
- destino sin permisos o sin espacio;
- colisión de assets no autorizada;
- fallo al escribir o mover atómicamente la salida;
- job o URL que no supera la validación de seguridad.

### 9.2 Advertencias que permiten guardar

- orden de columnas ambiguo;
- tabla degradada a párrafos;
- fuente sin metadata suficiente para inferir énfasis;
- imagen sin texto alternativo;
- link o asset omitido por datos inválidos.

Las advertencias se muestran antes de cerrar el flujo y permanecen asociadas al
documento abierto. Nunca se oculta una degradación relevante como si fuera una
conversión exacta.

### 9.3 Limpieza

Los jobs consumidos se eliminan al completar o cancelar. Al iniciar, MDViewer
elimina jobs huérfanos con más de 24 horas. Los directorios temporales de salida
se limpian después de un fallo y también en la recuperación del siguiente
inicio.

## 10. Seguridad y privacidad

- Procesamiento completamente local.
- Sin telemetría ni solicitudes de red en el camino de conversión.
- Preview HTML sanitizado y con navegación externa bloqueada por defecto.
- Esquema `mdviewer://` limitado a identificadores y acciones conocidas.
- Canonicalización de todas las rutas antes de leer o escribir.
- Prevención de path traversal en nombres de documentos y assets.
- Límites configurados para tamaño de entrada, cantidad de páginas y recursos,
  con errores explícitos en lugar de consumo ilimitado.
- Capacidades Tauri de mínimo privilegio.
- Dependencias Rust y frontend fijadas mediante lockfiles y auditadas en CI.

## 11. Portabilidad

El núcleo, CLI y aplicación desktop deben compilar en los tres sistemas desde
el comienzo. Los adapters de impresión son reemplazables:

- macOS v1: PDF Workflow.
- Windows posterior: impresora local IPP Everywhere basada en PAPPL y el driver
  IPP incluido en Windows. Un spike previo valida registro local y UX contra la
  versión de Windows soportada; un bloqueo de plataforma obliga a reabrir esta
  decisión antes de adoptar una Print Support Virtual Printer específica.
- Linux posterior: la misma impresora IPP Everywhere basada en PAPPL,
  descubierta por CUPS de forma driverless.

Cada adapter sólo crea un job local y activa MDViewer. No contiene lógica de
conversión ni de interfaz. Por lo tanto, portar la función universal no duplica
el producto.

## 12. Migración desde Swift

1. Congelar fixtures y comportamiento observable del MDViewer actual.
2. Mover la implementación Swift sin cambios semánticos a `legacy/macos-swift`.
3. Crear la aplicación Tauri y el núcleo Rust en paralelo.
4. Portar apertura, visualización, edición, guardado, preferencias y exportación.
5. Portar los convertidores locales incluidos en v1 al modelo intermedio y
   comparar resultados. El convertidor de YouTube permanece sólo en el legado
   hasta que exista un diseño posterior para importaciones con red explícita.
6. Alcanzar paridad de tests y pruebas manuales.
7. Crear un tag de la última versión Swift buildable.
8. Retirar el target Swift en un commit dedicado.

Durante la migración, `main` no debe quedar sin una aplicación buildable. Cada
milestone termina con al menos una implementación ejecutable y con sus gates
verdes.

## 13. Testing

### 13.1 Unitario y de contratos

- Modelo intermedio e invariantes.
- Agrupación de texto y orden de lectura.
- Detección de headings, listas, tablas, headers y footers.
- Escape GFM y referencias relativas.
- Escritura atómica, colisiones y cleanup.
- Validación de UUIDs, rutas y manifests.

### 13.2 Fixtures y golden tests

Cada fixture posee salida `.md`, assets y advertencias esperadas. Los cambios
se revisan como diff semántico. El corpus incluye:

- HTML semántico y malformado;
- PDF de una y varias columnas;
- tablas con y sin bordes;
- listas, links e imágenes;
- headers y footers repetidos;
- fuentes embebidas y sustituidas;
- PDFs sin texto y entradas corruptas.

### 13.3 Integración macOS

- instalar, detectar, reparar y desinstalar el workflow;
- invocar con MDViewer cerrado y abierto;
- cancelar antes y durante la conversión;
- sobrescribir de forma segura;
- actualizar la herramienta instalada;
- abrir el resultado en MDViewer;
- validar firma, notarización y Gatekeeper.

### 13.4 Corpus real

Se generan trabajos desde Safari, Mail, TextEdit, Preview y aplicaciones Office
disponibles. Los resultados se revisan contra una checklist de lectura, títulos,
listas, tablas, links e imágenes. Los defectos reproducibles se incorporan como
fixtures antes de corregirse.

### 13.5 CI

- Rust: formato, lint, tests, auditoría y builds por target.
- Frontend: lint, typecheck, unit tests y build.
- Desktop: smoke build en macOS, Windows y Linux.
- Release macOS: build `aarch64-apple-darwin`, firma, notarización, DMG y
  Gatekeeper.

## 14. Publicación y repositorios

- Repositorio primario: `https://github.com/mandalorianuy/mdviewer`.
- Visibilidad: pública.
- Licencia: MIT.
- Rama principal: `main`.
- OneDev se conserva como remote `onedev` y mirror secundario. Una caída
  temporal de OneDev se reporta como estado de mirror pendiente, sin cambiar la
  autoridad de GitHub.
- Los releases y checks públicos se publican en GitHub.

## 15. Criterios de éxito de v1

- MDViewer Tauri alcanza paridad acordada con la aplicación Swift y la versión
  Swift no se retira antes de demostrarla.
- Un usuario de macOS Apple Silicon instala o desinstala la acción universal sin
  privilegios administrativos.
- La acción aparece en el menú PDF de aplicaciones macOS compatibles.
- Un trabajo válido abre Guardar como…, produce GFM y assets coherentes y se
  abre en MDViewer.
- Cancelaciones y errores no dejan salidas parciales.
- PDFs sin texto se diagnostican como requerimiento de OCR.
- El motor, CLI y desktop pasan CI en macOS, Windows y Linux.
- El DMG Apple Silicon está firmado, notarizado y supera Gatekeeper.
- El repositorio público contiene licencia, documentación de build,
  contribución, arquitectura y limitaciones de fidelidad.

## 16. Evolución posterior

- v1.1: OCR local detrás del mismo contrato de extractor.
- Versión Windows: producto completo más adapter universal de impresión.
- Versión Linux: producto completo más Printer Application driverless.
- Integración de navegador: captura HTML directa para máxima fidelidad web.
- Intel macOS: sólo después de medir demanda y costo de soporte.
