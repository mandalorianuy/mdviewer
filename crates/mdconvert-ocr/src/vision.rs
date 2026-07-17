use objc2::{AnyThread, rc::autoreleasepool, runtime::AnyObject};
use objc2_foundation::{NSArray, NSData, NSDictionary};
use objc2_vision::{
    VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizeTextRequestRevision3,
    VNRequest, VNRequestTextRecognitionLevel,
};

use crate::{OcrError, OcrInput, OcrLine, OcrOutput, OcrRect};

pub(crate) fn recognize(input: OcrInput<'_>) -> Result<OcrOutput, OcrError> {
    autoreleasepool(|_| {
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        request.setAutomaticallyDetectsLanguage(true);
        // SAFETY: Revision 3 is available on the product's macOS 13 minimum and
        // is the first revision that implements automatic language detection.
        unsafe { request.setRevision(VNRecognizeTextRequestRevision3) };

        let data = NSData::with_bytes(input.bytes());
        let options = NSDictionary::<VNImageOption, AnyObject>::new();
        let handler = VNImageRequestHandler::initWithData_options(
            VNImageRequestHandler::alloc(),
            &data,
            &options,
        );
        let request_ref: &VNRequest = &request;
        let requests = NSArray::<VNRequest>::from_slice(&[request_ref]);
        handler
            .performRequests_error(&requests)
            .map_err(|_| OcrError::RecognitionFailed)?;

        let observations = request.results().ok_or(OcrError::RecognitionFailed)?;
        let mut lines = Vec::with_capacity(observations.len());
        for index in 0..observations.len() {
            let observation = observations.objectAtIndex(index);
            let candidates = observation.topCandidates(1);
            if candidates.is_empty() {
                continue;
            }
            let candidate = candidates.objectAtIndex(0);
            // SAFETY: Vision returned a live recognized-text observation. Its
            // superclass contract exposes a normalized immutable bounding box.
            let native_bounds = unsafe { observation.boundingBox() };
            let left = native_bounds.origin.x as f32;
            let width = native_bounds.size.width as f32;
            let height = native_bounds.size.height as f32;
            let top = 1.0 - native_bounds.origin.y as f32 - height;
            let bounds = normalized_rect(left, top, width, height)?;
            let text = candidate.string().to_string();
            if let Ok(line) = OcrLine::try_new(text, candidate.confidence(), bounds) {
                lines.push(line);
            }
        }
        Ok(OcrOutput::new(lines))
    })
}

fn normalized_rect(left: f32, top: f32, width: f32, height: f32) -> Result<OcrRect, OcrError> {
    let left = left.clamp(0.0, 1.0);
    let top = top.clamp(0.0, 1.0);
    let width = width.clamp(f32::EPSILON, 1.0 - left);
    let height = height.clamp(f32::EPSILON, 1.0 - top);
    OcrRect::new(left, top, width, height)
}
