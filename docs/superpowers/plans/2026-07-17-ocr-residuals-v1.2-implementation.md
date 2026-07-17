# OCR portable y captura universal v1.2 — plan de implementación

Autoridad de diseño:
`docs/superpowers/specs/2026-07-17-ocr-residuals-v1.2-design.md`.

Cada task requiere test rojo observable, cambio mínimo, test verde y evidencia
separada de implementación. No se modifica la release ni el tag `v1.1.0`.

## Task 1 — Contrato y selección de imágenes PDF

- Extender `OcrSource` para distinguir una imagen embebida de una página PDF.
- Agregar tests con motor falso para una página con texto digital e imagen,
  umbrales de elegibilidad, presupuestos, geometría y orden.
- Codificar el RGBA extraído por PDFium y aplicar OCR una sola vez por imagen.
- Conservar el asset y registrar intentos, no-text, unavailable y baja confianza.
- Suprimir sólo duplicados equivalentes con solapamiento espacial demostrado.

Gate: suite `mdconvert-pdf` completa y fixture mixto sin regresiones.

## Task 2 — Corpus y calidad OCR

- Versionar fixtures deterministas para contraste, inversión, rotación, idiomas y
  tamaños; definir tokens esperados en datos, no en heurísticas del test.
- Crear un evaluador común de recuperación de tokens y geometría.
- Mantener tests exactos con motor falso y tests reales nativos por backend.
- Documentar métricas, limitaciones y cualquier excepción aprobada.

Gate: 100 % en fixtures contractuales y al menos 95 % en el fixture real estable
de cada backend.

## Task 3 — Backend Windows nativo

- Agregar dependencias `windows` sólo para `target_os = "windows"`.
- Convertir la entrada validada a `SoftwareBitmap` y usar
  `Windows.Media.Ocr.OcrEngine`.
- Mapear idioma, líneas, palabras, confianza disponible y rectángulos al contrato
  neutral; redactar errores.
- Ejecutar test real y desktop smoke build en un runner Windows soportado.

Gate: backend real Windows, sin red ni runtime ML agregado, con CI verde nativa.

## Task 4 — Backend Linux Tesseract 5

- Elegir y fijar binding/library ABI, `libtesseract`, Leptonica y `traineddata`.
- Implementar reconocimiento por API, rectángulos y confianza de palabra/línea.
- Resolver `eng`/`spa` desde un recurso empaquetado y fallar claramente si falta.
- Agregar instalación reproducible en CI, test real y desktop smoke build Linux.
- Medir el incremento real del paquete antes de decidir bundle dinámico o
  AppImage/Flatpak runtime; no estimarlo por intuición.

Gate: backend real Linux sin subprocess ni descargas durante conversión, con
recibo de versiones/licencias y CI verde.

## Task 5 — Print Support App v4 para Windows

- Crear un spike separado MSIX/UWP que declare la extensión de virtual printer.
- Probar registro/desregistro, aparición en Print, recepción de PDL y handoff
  local autenticado a MDViewer.
- Definir identidad de paquete, protocolo/ACL, cleanup y modelo de actualización.
- Convertir el spike en adapter productivo sólo con evidencia en Windows real.

Gate: install/print/save/uninstall end-to-end; compilación cruzada no cuenta.

## Task 6 — Printer Application Linux

- Crear adapter PAPPL/CUPS mínimo y una cola local IPP Everywhere.
- Recibir el trabajo con límites, persistirlo de forma segura y abrir MDViewer.
- Probar instalación, impresión desde una app externa, handoff y desinstalación en
  una distribución objetivo.

Gate: install/print/save/uninstall end-to-end en Linux real.

## Task 7 — Integración, documentación y versiones

- Actualizar README, CLI, arquitectura, seguridad, parity manifest y release
  notes con soporte comprobado y limitaciones restantes.
- Coordinar versiones como una release posterior a `1.1.0`.
- Ejecutar `./scripts/verify-workspace.sh`, auditorías y smoke manual del flujo
  Guardar como Markdown.
- Publicar branch y PR en GitHub; revisar el head exacto y sincronizar OneDev.

Gate: PR aprobable con CI exact-head verde y worktree raíz sin cambios ajenos.

## Task 8 — Release macOS firmada y notarizada

- Construir desde el commit integrado y auditar arm64-only.
- Firmar con Developer ID, crear DMG, notarizar, staple y ejecutar `spctl`.
- Validar instalación limpia y Print Workflow end-to-end.
- Verificar receipt/checksums y publicar nueva GitHub Release; sincronizar tag en
  OneDev sin alterar `v1.1.0`.

Gate: el receipt marca `signed`, `notarized` y `publishable`; el artefacto exacto
publicado coincide con el checksum verificado.
