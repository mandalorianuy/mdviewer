use windows::{
    Graphics::Imaging::BitmapDecoder,
    Media::Ocr::OcrEngine as WindowsOcrEngine,
    Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
};

use crate::{OcrError, OcrInput, OcrLine, OcrOutput, OcrRect};

const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;

pub(crate) fn recognize(input: OcrInput<'_>) -> Result<OcrOutput, OcrError> {
    let _winrt = initialize_winrt()?;
    let max_dimension = WindowsOcrEngine::MaxImageDimension().map_err(recognition_error)?;
    if input.width() > max_dimension || input.height() > max_dimension {
        return Err(OcrError::InvalidInput);
    }

    let stream = InMemoryRandomAccessStream::new().map_err(recognition_error)?;
    let output_stream = stream.GetOutputStreamAt(0).map_err(recognition_error)?;
    let writer = DataWriter::CreateDataWriter(&output_stream).map_err(recognition_error)?;
    writer
        .WriteBytes(input.bytes())
        .map_err(recognition_error)?;
    writer
        .StoreAsync()
        .and_then(|operation| operation.join())
        .map_err(recognition_error)?;
    writer
        .FlushAsync()
        .and_then(|operation| operation.join())
        .map_err(recognition_error)?;
    writer.DetachStream().map_err(recognition_error)?;
    stream.Seek(0).map_err(recognition_error)?;

    let decoder = BitmapDecoder::CreateAsync(&stream)
        .and_then(|operation| operation.join())
        .map_err(recognition_error)?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .and_then(|operation| operation.join())
        .map_err(recognition_error)?;
    let engine =
        WindowsOcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| OcrError::Unavailable)?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .and_then(|operation| operation.join())
        .map_err(recognition_error)?;

    let mut output = Vec::new();
    for line in &result.Lines().map_err(recognition_error)? {
        let text = line.Text().map_err(recognition_error)?.to_string();
        let words = line.Words().map_err(recognition_error)?;
        let mut left = f32::INFINITY;
        let mut top = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        let mut bottom = f32::NEG_INFINITY;
        for word in &words {
            let rect = word.BoundingRect().map_err(recognition_error)?;
            left = left.min(rect.X);
            top = top.min(rect.Y);
            right = right.max(rect.X + rect.Width);
            bottom = bottom.max(rect.Y + rect.Height);
        }
        if !left.is_finite() || right <= left || bottom <= top {
            continue;
        }
        let bounds = normalized_rect(left, top, right - left, bottom - top, &input)?;
        // Windows.Media.Ocr does not expose a confidence value. 0.5 means
        // unknown/neutral in the provider-neutral contract and avoids inventing
        // either high or low confidence.
        if let Ok(line) = OcrLine::try_new(text, 0.5, bounds) {
            output.push(line);
        }
    }
    Ok(OcrOutput::new(output))
}

struct WinRtGuard {
    owns_initialization: bool,
}

impl Drop for WinRtGuard {
    fn drop(&mut self) {
        if self.owns_initialization {
            // SAFETY: this guard is dropped on the same thread where the
            // matching successful RoInitialize call was made.
            unsafe { RoUninitialize() };
        }
    }
}

fn initialize_winrt() -> Result<WinRtGuard, OcrError> {
    // SAFETY: initializes WinRT for the current thread. An existing STA is also
    // valid for the APIs used here and is reported as RPC_E_CHANGED_MODE.
    match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        Ok(()) => Ok(WinRtGuard {
            owns_initialization: true,
        }),
        Err(error) if error.code().0 == RPC_E_CHANGED_MODE => Ok(WinRtGuard {
            owns_initialization: false,
        }),
        Err(_) => Err(OcrError::Unavailable),
    }
}

fn normalized_rect(
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    input: &OcrInput<'_>,
) -> Result<OcrRect, OcrError> {
    let image_width = input.width() as f32;
    let image_height = input.height() as f32;
    let left = (left / image_width).clamp(0.0, 1.0);
    let top = (top / image_height).clamp(0.0, 1.0);
    let width = (width / image_width).clamp(f32::EPSILON, 1.0 - left);
    let height = (height / image_height).clamp(f32::EPSILON, 1.0 - top);
    OcrRect::new(left, top, width, height)
}

fn recognition_error(_: windows::core::Error) -> OcrError {
    OcrError::RecognitionFailed
}
