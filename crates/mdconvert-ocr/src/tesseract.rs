use std::collections::BTreeMap;

use leptess::LepTess;

use crate::{OcrError, OcrInput, OcrLine, OcrOutput, OcrRect};

const LANGUAGES: &str = "eng+spa";

pub(crate) fn recognize(input: OcrInput<'_>) -> Result<OcrOutput, OcrError> {
    let mut engine = LepTess::new(None, LANGUAGES).map_err(|_| OcrError::Unavailable)?;
    engine
        .set_image_from_mem(input.bytes())
        .map_err(|_| OcrError::RecognitionFailed)?;
    engine.set_fallback_source_resolution(300);
    if engine.recognize() != 0 {
        return Err(OcrError::RecognitionFailed);
    }
    let tsv = engine
        .get_tsv_text(0)
        .map_err(|_| OcrError::RecognitionFailed)?;
    parse_tsv(&tsv, input.width(), input.height())
}

#[derive(Debug)]
struct TsvLine {
    text: Vec<String>,
    confidence_sum: f32,
    word_count: u32,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

fn parse_tsv(tsv: &str, image_width: u32, image_height: u32) -> Result<OcrOutput, OcrError> {
    let mut lines = BTreeMap::<(u32, u32, u32, u32), TsvLine>::new();
    for row in tsv.lines().skip(1) {
        let fields = row.splitn(12, '\t').collect::<Vec<_>>();
        if fields.len() != 12 || fields[0] != "5" {
            continue;
        }
        let text = fields[11].trim();
        if text.is_empty() {
            continue;
        }
        let values = fields[1..10]
            .iter()
            .map(|value| value.parse::<u32>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| OcrError::RecognitionFailed)?;
        let confidence = fields[10]
            .parse::<f32>()
            .map_err(|_| OcrError::RecognitionFailed)?;
        if !confidence.is_finite() || confidence < 0.0 {
            continue;
        }
        let key = (values[0], values[1], values[2], values[3]);
        let left = values[5].min(image_width);
        let top = values[6].min(image_height);
        let right = left.saturating_add(values[7]).min(image_width);
        let bottom = top.saturating_add(values[8]).min(image_height);
        if right <= left || bottom <= top {
            continue;
        }
        lines
            .entry(key)
            .and_modify(|line| {
                line.text.push(text.into());
                line.confidence_sum += confidence;
                line.word_count += 1;
                line.left = line.left.min(left);
                line.top = line.top.min(top);
                line.right = line.right.max(right);
                line.bottom = line.bottom.max(bottom);
            })
            .or_insert_with(|| TsvLine {
                text: vec![text.into()],
                confidence_sum: confidence,
                word_count: 1,
                left,
                top,
                right,
                bottom,
            });
    }

    let mut output = Vec::with_capacity(lines.len());
    for line in lines.into_values() {
        let bounds = OcrRect::new(
            line.left as f32 / image_width as f32,
            line.top as f32 / image_height as f32,
            (line.right - line.left) as f32 / image_width as f32,
            (line.bottom - line.top) as f32 / image_height as f32,
        )?;
        let confidence = (line.confidence_sum / line.word_count as f32 / 100.0).clamp(0.0, 1.0);
        output.push(OcrLine::try_new(line.text.join(" "), confidence, bounds)?);
    }
    Ok(OcrOutput::new(output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_words_are_grouped_into_lines_with_normalized_geometry_and_confidence() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
5\t1\t1\t1\t1\t1\t10\t20\t40\t10\t90.0\tHola\n\
5\t1\t1\t1\t1\t2\t55\t20\t35\t10\t80.0\tmundo\n\
5\t1\t1\t1\t2\t1\t10\t50\t30\t10\t70.0\tNext\n";

        let output = parse_tsv(tsv, 100, 100).unwrap();

        assert_eq!(output.lines().len(), 2);
        assert_eq!(output.lines()[0].text(), "Hola mundo");
        assert!((output.lines()[0].confidence() - 0.85).abs() < f32::EPSILON);
        assert_eq!(
            output.lines()[0].bounds(),
            OcrRect::new(0.1, 0.2, 0.8, 0.1).unwrap()
        );
        assert_eq!(output.lines()[1].text(), "Next");
    }
}
