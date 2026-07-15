use std::{
    env,
    path::{Path, PathBuf},
    sync::Mutex,
};

use mdconvert_core::ConversionError;
use pdfium_render::prelude::{Pdfium, PdfiumError};

const PDFIUM_DYNAMIC_LIB_PATH: &str = "PDFIUM_DYNAMIC_LIB_PATH";

static LOADED_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub(crate) fn load_pdfium() -> Result<Pdfium, ConversionError> {
    let configured =
        env::var_os(PDFIUM_DYNAMIC_LIB_PATH).ok_or_else(|| ConversionError::ConversionFailed {
            message: format!("{PDFIUM_DYNAMIC_LIB_PATH} is not set"),
        })?;
    let configured = PathBuf::from(configured);
    let canonical = canonical_library_path(&configured)?;

    let mut loaded_path = LOADED_PATH
        .lock()
        .map_err(|_| ConversionError::ConversionFailed {
            message: "PDFium binding state is unavailable".into(),
        })?;

    if let Some(existing) = loaded_path.as_ref() {
        if existing != &canonical {
            return Err(ConversionError::ConversionFailed {
                message: format!(
                    "PDFium is already bound to {}, not {}",
                    existing.display(),
                    canonical.display()
                ),
            });
        }
        return Ok(Pdfium::default());
    }

    match Pdfium::bind_to_library(&canonical) {
        Ok(bindings) => {
            let pdfium = Pdfium::new(bindings);
            *loaded_path = Some(canonical);
            Ok(pdfium)
        }
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
            Err(ConversionError::ConversionFailed {
                message: "PDFium bindings were initialized outside mdconvert-pdf; refusing to reuse an unverified library".into(),
            })
        }
        Err(error) => Err(binding_error(&canonical, error)),
    }
}

fn canonical_library_path(configured: &Path) -> Result<PathBuf, ConversionError> {
    let metadata =
        std::fs::metadata(configured).map_err(|source| ConversionError::ConversionFailed {
            message: format!(
                "could not access PDFium at {}: {source}",
                configured.display()
            ),
        })?;
    if !metadata.is_file() {
        return Err(ConversionError::ConversionFailed {
            message: format!(
                "{PDFIUM_DYNAMIC_LIB_PATH} does not name a regular file: {}",
                configured.display()
            ),
        });
    }
    configured
        .canonicalize()
        .map_err(|source| ConversionError::ConversionFailed {
            message: format!(
                "could not resolve PDFium path {}: {source}",
                configured.display()
            ),
        })
}

fn binding_error(path: &Path, error: PdfiumError) -> ConversionError {
    ConversionError::ConversionFailed {
        message: format!("could not load PDFium from {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_library_is_a_typed_conversion_failure() {
        assert!(matches!(
            canonical_library_path(Path::new("/definitely/absent/libpdfium.dylib")),
            Err(ConversionError::ConversionFailed { .. })
        ));
    }
}
