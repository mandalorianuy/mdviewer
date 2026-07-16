use std::{
    env,
    path::{Path, PathBuf},
    sync::Mutex,
};

use mdconvert_core::ConversionError;
use pdfium_render::prelude::{Pdfium, PdfiumError};

const PDFIUM_DYNAMIC_LIB_PATH: &str = "PDFIUM_DYNAMIC_LIB_PATH";

static LOADED_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static CONFIGURED_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn configure_pdfium_library_path(path: impl AsRef<Path>) -> Result<(), ConversionError> {
    let canonical = canonical_library_path(path.as_ref())?;
    let mut configured = CONFIGURED_PATH
        .lock()
        .map_err(|_| ConversionError::ConversionFailed {
            message: "PDFium configuration state is unavailable".into(),
        })?;
    if let Some(existing) = configured.as_ref() {
        return if existing == &canonical {
            Ok(())
        } else {
            Err(ConversionError::ConversionFailed {
                message: "PDFium is already configured with a different verified runtime".into(),
            })
        };
    }
    let loaded = LOADED_PATH
        .lock()
        .map_err(|_| ConversionError::ConversionFailed {
            message: "PDFium binding state is unavailable".into(),
        })?;
    if loaded
        .as_ref()
        .is_some_and(|existing| existing != &canonical)
    {
        return Err(ConversionError::ConversionFailed {
            message: "PDFium is already bound to a different verified runtime".into(),
        });
    }
    drop(loaded);
    *configured = Some(canonical);
    Ok(())
}

pub(crate) fn load_pdfium() -> Result<Pdfium, ConversionError> {
    let canonical = configured_library_path()?;

    let mut loaded_path = LOADED_PATH
        .lock()
        .map_err(|_| ConversionError::ConversionFailed {
            message: "PDFium binding state is unavailable".into(),
        })?;

    if let Some(existing) = loaded_path.as_ref() {
        if existing != &canonical {
            return Err(ConversionError::ConversionFailed {
                message: "PDFium is already bound to a different verified runtime".into(),
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

fn configured_library_path() -> Result<PathBuf, ConversionError> {
    if let Some(configured) = CONFIGURED_PATH
        .lock()
        .map_err(|_| ConversionError::ConversionFailed {
            message: "PDFium configuration state is unavailable".into(),
        })?
        .clone()
    {
        return Ok(configured);
    }
    let configured =
        env::var_os(PDFIUM_DYNAMIC_LIB_PATH).ok_or(ConversionError::PdfiumUnavailable)?;
    canonical_library_path(Path::new(&configured))
}

fn canonical_library_path(configured: &Path) -> Result<PathBuf, ConversionError> {
    let metadata = std::fs::metadata(configured).map_err(|_| ConversionError::PdfiumUnavailable)?;
    if !metadata.is_file() {
        return Err(ConversionError::PdfiumUnavailable);
    }
    configured
        .canonicalize()
        .map_err(|_| ConversionError::PdfiumUnavailable)
}

fn binding_error(_path: &Path, _error: PdfiumError) -> ConversionError {
    ConversionError::PdfiumUnavailable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_library_is_typed_pdfium_unavailable() {
        assert!(matches!(
            canonical_library_path(Path::new("/definitely/absent/libpdfium.dylib")),
            Err(ConversionError::PdfiumUnavailable)
        ));
    }
}
