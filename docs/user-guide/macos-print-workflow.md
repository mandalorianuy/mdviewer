# macOS: Guardar como Markdown desde Imprimir

MDViewer puede instalar un PDF Service por usuario. En MDViewer, abrí **Integrations** y usá
**Install**. El estado debe cambiar de `not installed` a `installed`.

La integración se instala como un alias nativo a la aplicación firmada en:

```text
~/Library/PDF Services/Guardar como Markdown con MDViewer
```

MDViewer resuelve el alias sin mostrar interfaz y comprueba el bundle ID, el Team ID, la firma de
código y el SHA-256 del ejecutable de la aplicación antes de mostrarlo como `installed`. Un alias a
una versión anterior firmada aparece como `outdated` y ofrece **Repair**. Un elemento `invalid` se
preserva sin ofrecer una acción destructiva. **Uninstall** requiere confirmación y elimina solamente
el alias exacto administrado por MDViewer; nunca elimina `PDF Services` ni otros workflows. Antes
de reparar o desinstalar, MDViewer mueve el objeto observado a una cuarentena única sin sobrescribir,
vuelve a comprobar su identidad y contenido, y aborta restaurándolo si el destino cambió durante la
operación.

## Uso

1. Abrí un documento en una aplicación macOS, por ejemplo TextEdit.
2. Elegí **Archivo → Imprimir**.
3. En el menú **PDF**, elegí **Guardar como Markdown con MDViewer…**.
4. macOS entrega el PDF temporal directamente a la aplicación MDViewer mediante un evento nativo
   de apertura.
5. MDViewer copia y sincroniza el PDF en su almacén privado antes de responder al evento, y muestra
   el selector **Guardar como**.
6. Elegí un destino para convertir el PDF a GitHub Flavored Markdown, o cancelá.

El PDF Service no ejecuta un helper ni intenta escribir en el almacenamiento de MDViewer desde
`printtool`. La aplicación valida el PDF, crea un trabajo durable y envía al WebView solamente un
UUID opaco. No elige el destino ni convierte hasta que el usuario actúa en MDViewer. Cancelar el
selector no crea Markdown ni assets y elimina el trabajo privado reclamado.

El flujo es local: no habilita red, no envía rutas de archivos al WebView y no necesita
credenciales. Si MDViewer estaba cerrado, la cola nativa conserva los UUID hasta que el frontend
está listo; si estaba abierto, el mismo evento se procesa inmediatamente.

La aplicación firmada incluye su versión fijada de PDFium para realizar la conversión local. No
requiere instalar una biblioteca aparte ni configurar variables de entorno.

## Solución de problemas

- Si el estado es `outdated`, usá **Repair**.
- Si es `invalid`, MDViewer preservó un archivo, enlace o alias que no pudo verificar. Retiralo
  manualmente sólo si confirmaste que no pertenece a otra aplicación; luego usá **Install**.
- Si la opción no aparece en el menú PDF, cerrá y volvé a abrir la aplicación desde la que imprimís
  después de instalar la integración.
- Si la reparación o desinstalación falla, verificá que el destino siga siendo el alias nativo
  firmado que instaló MDViewer. Los enlaces simbólicos y archivos ajenos nunca se siguen ni borran.
