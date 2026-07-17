# Impresora virtual macOS para Chrome

## Problema

El PDF Service `Guardar como Markdown con MDViewer` aparece únicamente en el menú PDF del diálogo
nativo de macOS. Chrome usa su propia vista de impresión y sólo enumera destinos registrados en
CUPS, por lo que el servicio actual no puede aparecer junto a las impresoras físicas. Un diálogo
`Guardar como` tampoco admite conversores arbitrarios.

## Decisión

MDViewer conserva el PDF Service como compatibilidad y agrega una cola CUPS por usuario llamada
`MDViewer — Guardar como Markdown`. La cola usa IPP Everywhere y apunta a un `ippeveprinter` local,
escuchando solamente en loopback. Un LaunchAgent de usuario mantiene ese servidor disponible. Cada
trabajo acepta exclusivamente `application/pdf` y se entrega al bundle `com.mdviewer.desktop`, que
lo copia a su almacén durable antes de mostrar el selector Guardar como y ejecutar la conversión
local existente.

El PDF continúa siendo el límite de entrada porque es el formato que el sistema de impresión
entrega de forma uniforme desde aplicaciones arbitrarias. HTML sólo es preferible cuando una app
lo ofrece explícitamente; no forma parte del contrato universal de impresión.

## Componentes administrados

- Cola CUPS: `MDViewer_Save_as_Markdown`, URI `ipp://localhost:8631/ipp/print`, modelo
  `everywhere`.
- LaunchAgent: `~/Library/LaunchAgents/com.mdviewer.desktop.virtual-printer.plist`.
- Helper y spool privados:
  `~/Library/Application Support/com.mdviewer.desktop/Virtual Printer/`.
- Servidor del sistema: `/usr/bin/ippeveprinter`, sin listener de red externa ni formularios web.

El servidor se ejecuta con DNS-SD desactivado (`-r off`): Chrome recibe una sola identidad desde la
cola CUPS explícita y no crea un segundo destino temporal desde Bonjour.

La app sólo considera `installed` el conjunto exacto que administra. Reparar o desinstalar requiere
que la cola conserve el URI esperado y que el plist/helper coincidan con el contenido generado por
esta versión. Objetos ajenos o no verificables quedan en estado `invalid` y se preservan.

## Ciclo de vida

`Install` publica primero los archivos con permisos privados, activa el LaunchAgent, comprueba que
el endpoint IPP responde y recién entonces registra la cola CUPS. Si un paso falla, revierte sólo
los componentes que creó. `Repair` reemplaza únicamente una instalación MDViewer verificable.
`Uninstall` retira la cola exacta, desactiva el LaunchAgent y conserva cualquier objeto que haya
cambiado de identidad o contenido.

La interfaz muestra el PDF Service y la impresora virtual como integraciones separadas. La segunda
explica que es la opción necesaria para Chrome y otras aplicaciones que no usan el diálogo nativo.

## Aceptación

1. Después de instalar, Chrome muestra `MDViewer — Guardar como Markdown` en **See more…**.
2. Imprimir una página de Chrome abre MDViewer y llega al selector Guardar como con un PDF durable.
3. Cancelar no crea Markdown y los temporales se limpian de forma acotada.
4. Desinstalar retira sólo la cola, LaunchAgent y archivos exactos de MDViewer.
5. El PDF Service nativo continúa funcionando.
6. Todo el procesamiento permanece local y el endpoint escucha únicamente en loopback.
