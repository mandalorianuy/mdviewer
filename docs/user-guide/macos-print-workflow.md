# macOS: Guardar como Markdown desde Imprimir

MDViewer puede instalar un PDF Workflow por usuario. En MDViewer, abrí **Integrations** y usá
**Install**. El estado debe cambiar de `not installed` a `installed`.

El workflow se instala como un único ejecutable en:

```text
~/Library/PDF Services/Guardar como Markdown con MDViewer
```

MDViewer comprueba la versión, el SHA-256 y la firma de código del ejecutable antes de mostrarlo
como `installed`. `outdated` ofrece **Repair** para actualizarlo; `invalid` ofrece **Repair**, pero
una instalación que no tenga el marcador de MDViewer se preserva y la reparación falla de forma
segura. **Uninstall** requiere confirmación y elimina solamente el archivo exacto de MDViewer. No
elimina la carpeta `PDF Services` ni otros workflows.

## Uso

1. Abrí un documento en una aplicación macOS, por ejemplo TextEdit.
2. Elegí **Archivo → Imprimir**.
3. En el menú **PDF**, elegí **Guardar como Markdown con MDViewer…**.
4. MDViewer se abre y muestra el selector **Guardar como**.
5. Elegí un destino para convertir el PDF a GitHub Flavored Markdown, o cancelá.

El workflow solamente valida y copia el PDF a un trabajo privado y durable, y solicita a Launch
Services abrir `mdviewer://print/<uuid>`. No elige el destino ni convierte el documento. Esas
acciones permanecen en MDViewer, donde requieren una selección explícita del usuario. Cancelar el
selector no crea Markdown ni assets.

El flujo es local: no habilita red, no envía rutas de archivos al WebView y no necesita
credenciales. Un error de persistencia no abre MDViewer; un error al abrir MDViewer deja el trabajo
privado listo para que la aplicación lo recupere al iniciarse.

## Solución de problemas

- Si el estado es `outdated`, usá **Repair**.
- Si es `invalid`, usá **Repair** solamente si el archivo es el workflow administrado por MDViewer.
  MDViewer no reemplaza archivos sin su marcador.
- Si la opción no aparece en el menú PDF, cerrá y volvé a abrir la aplicación desde la que imprimís
  después de instalar el workflow.
- Si la reparación o desinstalación falla, verificá que el destino no sea un enlace simbólico ni un
  archivo ajeno. MDViewer los preserva intencionalmente.
