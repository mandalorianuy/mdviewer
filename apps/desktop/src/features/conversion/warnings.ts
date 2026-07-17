const warningMessages: Record<string, string> = {
  ambiguous_reading_order: "El orden de lectura puede ser ambiguo.",
  table_degraded: "La tabla se simplificó durante la conversión.",
  font_metadata_insufficient: "No hubo suficiente información tipográfica.",
  missing_image_alt: "Falta texto alternativo en una imagen.",
  invalid_link_skipped: "Se omitió un enlace inválido.",
  invalid_asset_skipped: "Se omitió un recurso inválido.",
  external_asset_skipped: "Se omitió un recurso externo.",
  external_link_skipped: "Se omitió un enlace externo.",
  additional_archive_entries_skipped: "Se omitieron entradas adicionales del archivo.",
  ocr_deferred: "El OCR local no está disponible en esta plataforma.",
  ocr_no_text_found: "El OCR local terminó, pero no encontró texto.",
  ocr_low_confidence: "Se conservó texto reconocido con baja confianza; conviene revisarlo.",
  unknown_warning: "La conversión terminó con una advertencia.",
};

export function warningMessage(code: string): string {
  return warningMessages[code] ?? warningMessages.unknown_warning;
}
