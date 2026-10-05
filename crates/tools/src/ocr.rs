use image::RgbaImage;
use serde::{Deserialize, Serialize};
use windows::{
    Globalization::Language,
    Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Security::Cryptography::CryptographicBuffer,
    core::HSTRING,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub bounds: [f32; 4],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Recognition {
    pub text: String,
    pub words: Vec<Word>,
    pub language: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextSelection {
    pub text: String,
    pub bounds: Vec<[f32; 4]>,
}

impl Recognition {
    /// Test whether a point in the original image is inside recognized text.
    /// Keep the edges consistent with click selection so blank areas can be
    /// reserved for moving a capture or a pinned image.
    pub fn hit_test(&self, point: [f32; 2]) -> bool {
        let [x, y] = point;
        if !point.iter().all(|value| value.is_finite()) {
            return false;
        }
        self.words.iter().any(|word| {
            let [left, top, width, height] = word.bounds;
            valid_word(word) && x >= left && x <= left + width && y >= top && y <= top + height
        })
    }

    /// Select in the original image's coordinates. A click selects a whole word;
    /// dragging approximates character positions by dividing its OCR box evenly.
    pub fn select(&self, bounds: [f32; 4]) -> TextSelection {
        if !bounds.iter().all(|value| value.is_finite()) {
            return TextSelection::default();
        }
        let [x, y, width, height] = bounds;
        let left = x.min(x + width);
        let right = x.max(x + width);
        let top = y.min(y + height);
        let bottom = y.max(y + height);
        let click = width == 0.0 && height == 0.0;
        let mut selected = TextSelection::default();
        for word in &self.words {
            let [wx, wy, ww, wh] = word.bounds;
            if !valid_word(word) {
                continue;
            }
            let horizontal = if left == right {
                left >= wx && left <= wx + ww
            } else {
                left < wx + ww && right > wx
            };
            let vertical = if top == bottom {
                top >= wy && top <= wy + wh
            } else {
                top < wy + wh && bottom > wy
            };
            if !horizontal || !vertical {
                continue;
            }
            if click {
                selected.push(&word.text, word.bounds);
                break;
            }
            let chars: Vec<char> = word.text.chars().collect();
            let advance = ww / chars.len() as f32;
            let first = (((left - wx) / advance).floor().max(0.0) as usize).min(chars.len() - 1);
            let last = if left == right {
                first + 1
            } else {
                (((right - wx) / advance).ceil().max(0.0) as usize).clamp(first + 1, chars.len())
            };
            let text: String = chars[first..last].iter().collect();
            selected.push(
                &text,
                [
                    wx + first as f32 * advance,
                    wy,
                    (last - first) as f32 * advance,
                    wh,
                ],
            );
        }
        selected
    }

    pub fn select_all(&self) -> TextSelection {
        let mut selected = TextSelection::default();
        for word in &self.words {
            if valid_word(word) {
                selected.push(&word.text, word.bounds);
            }
        }
        if selected.bounds.is_empty() {
            selected.text = self.text.clone();
        }
        selected
    }
}

impl TextSelection {
    fn push(&mut self, text: &str, bounds: [f32; 4]) {
        if let Some(previous) = self.bounds.last() {
            let same_line =
                previous[1].max(bounds[1]) < (previous[1] + previous[3]).min(bounds[1] + bounds[3]);
            if !same_line {
                self.text.push('\n');
            } else if self
                .text
                .chars()
                .last()
                .zip(text.chars().next())
                .is_some_and(|(previous, next)| needs_space(previous, next))
            {
                self.text.push(' ');
            }
        }
        self.text.push_str(text);
        self.bounds.push(bounds);
    }
}

fn valid_word(word: &Word) -> bool {
    !word.text.trim().is_empty()
        && word.bounds.iter().all(|value| value.is_finite())
        && word.bounds[2] > 0.0
        && word.bounds[3] > 0.0
        && (word.bounds[0] + word.bounds[2]).is_finite()
        && (word.bounds[1] + word.bounds[3]).is_finite()
}

fn is_cjk(c: char) -> bool {
    ('\u{3400}'..='\u{9fff}').contains(&c)
}

fn needs_space(previous: char, next: char) -> bool {
    let cjk_punctuation =
        |c| ('\u{3000}'..='\u{303f}').contains(&c) || ('\u{ff01}'..='\u{ff65}').contains(&c);
    !((is_cjk(previous) || cjk_punctuation(previous)) && (is_cjk(next) || cjk_punctuation(next))
        || ",.;:!?%)]}".contains(next)
        || "([{".contains(previous))
}

pub fn normalize(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            if *c == ' '
                && i > 0
                && i + 1 < chars.len()
                && is_cjk(chars[i - 1])
                && is_cjk(chars[i + 1])
            {
                None
            } else {
                Some(*c)
            }
        })
        .collect()
}

pub fn languages() -> Result<Vec<String>, String> {
    OcrEngine::AvailableRecognizerLanguages()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|language| {
            language
                .LanguageTag()
                .map(|s| s.to_string())
                .map_err(|e| e.to_string())
        })
        .collect()
}

pub fn recognize(image: &RgbaImage) -> Result<Recognition, String> {
    let supported = languages()?;
    let chosen = supported.iter().find(|s| s.starts_with("zh-Hans"))
        .or_else(|| supported.iter().find(|s| s.starts_with("zh")))
        .or_else(|| supported.iter().find(|s| s.starts_with("en")))
        .ok_or("未安装中英文 OCR 语言包。请在 Windows 设置的语言选项中安装文字识别组件。识别过程完全离线。")?;
    let max = OcrEngine::MaxImageDimension().map_err(|e| e.to_string())?;
    let scale = (max as f32 / image.width().max(image.height()) as f32).min(1.0);
    let scaled = if scale < 1.0 {
        image::imageops::resize(
            image,
            (image.width() as f32 * scale).max(1.0) as u32,
            (image.height() as f32 * scale).max(1.0) as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image.clone()
    };
    let mut bgra = scaled.as_raw().clone();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    let result = (|| -> windows::core::Result<Recognition> {
        let engine = OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&HSTRING::from(
            chosen.as_str(),
        ))?)?;
        let buffer = CryptographicBuffer::CreateFromByteArray(&bgra)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            scaled.width() as i32,
            scaled.height() as i32,
        )?;
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let mut words = vec![];
        for line in result.Lines()? {
            for word in line.Words()? {
                let rect = word.BoundingRect()?;
                words.push(Word {
                    text: word.Text()?.to_string(),
                    bounds: [
                        rect.X / scale,
                        rect.Y / scale,
                        rect.Width / scale,
                        rect.Height / scale,
                    ],
                });
            }
        }
        Ok(Recognition {
            text: normalize(&result.Text()?.to_string()),
            words,
            language: chosen.clone(),
        })
    })();
    result.map_err(|e| format!("本机离线 OCR 不可用：{e}。不会改用云端识别。"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recognition(words: &[(&str, [f32; 4])]) -> Recognition {
        Recognition {
            text: String::new(),
            words: words
                .iter()
                .map(|(text, bounds)| Word {
                    text: (*text).into(),
                    bounds: *bounds,
                })
                .collect(),
            language: "zh-Hans-CN".into(),
        }
    }

    #[test]
    fn click_selects_a_word_and_empty_space_selects_nothing() {
        let ocr = recognition(&[("Hello", [10.0, 20.0, 50.0, 20.0])]);
        assert_eq!(
            ocr.select([32.0, 30.0, 0.0, 0.0]),
            TextSelection {
                text: "Hello".into(),
                bounds: vec![[10.0, 20.0, 50.0, 20.0]],
            }
        );
        assert_eq!(ocr.select([70.0, 30.0, 0.0, 0.0]), TextSelection::default());
        assert_eq!(
            ocr.select([12.0, 50.0, 30.0, 10.0]),
            TextSelection::default()
        );
    }

    #[test]
    fn hit_testing_matches_click_selection_at_word_edges_and_in_blank_gaps() {
        let ocr = recognition(&[
            ("Hello", [10.5, 20.5, 50.0, 20.0]),
            ("文字", [80.0, 20.0, 40.0, 20.0]),
        ]);
        for point in [
            [10.5, 20.5],
            [60.5, 40.5],
            [30.0, 30.0],
            [80.0, 20.0],
            [120.0, 40.0],
            [10.0, 30.0],
            [60.75, 30.0],
            [70.0, 30.0],
            [100.0, 41.0],
            [f32::NAN, 30.0],
            [30.0, f32::INFINITY],
        ] {
            assert_eq!(
                ocr.hit_test(point),
                !ocr.select([point[0], point[1], 0.0, 0.0]).bounds.is_empty(),
                "point {point:?} should follow click selection"
            );
        }
        assert!(ocr.hit_test([30.0, 30.0]));
        assert!(!ocr.hit_test([70.0, 30.0]));
    }

    #[test]
    fn hit_testing_ignores_blank_text_and_malformed_word_boxes() {
        let ocr = recognition(&[
            (" \t", [0.0, 0.0, 10.0, 10.0]),
            ("zero", [20.0, 0.0, 0.0, 10.0]),
            ("negative", [40.0, 0.0, 10.0, -10.0]),
            ("bad", [f32::NAN, 0.0, 10.0, 10.0]),
            ("overflow", [f32::MAX, 0.0, f32::MAX, 10.0]),
        ]);
        for point in [[5.0, 5.0], [20.0, 5.0], [45.0, 0.0], [f32::MAX, 5.0]] {
            assert!(!ocr.hit_test(point));
        }
        assert_eq!(ocr.select_all(), TextSelection::default());
    }

    #[test]
    fn dragging_selects_partial_unicode_text_and_matching_character_boxes() {
        let ocr = recognition(&[
            ("Hello", [10.0, 20.0, 50.0, 20.0]),
            ("截图文字", [80.0, 20.0, 80.0, 20.0]),
        ]);
        assert_eq!(
            ocr.select([20.0, 25.0, 20.0, 10.0]),
            TextSelection {
                text: "el".into(),
                bounds: vec![[20.0, 20.0, 20.0, 20.0]],
            }
        );
        assert_eq!(
            ocr.select([100.0, 25.0, 40.0, 10.0]),
            TextSelection {
                text: "图文".into(),
                bounds: vec![[100.0, 20.0, 40.0, 20.0]],
            }
        );
        assert_eq!(
            ocr.select([140.0, 35.0, -40.0, -10.0]),
            ocr.select([100.0, 25.0, 40.0, 10.0])
        );
    }

    #[test]
    fn zero_height_drag_selects_the_text_on_its_line() {
        let ocr = recognition(&[("Hello", [10.0, 20.0, 50.0, 20.0])]);
        assert_eq!(ocr.select([20.0, 30.0, 20.0, 0.0]).text, "el");
        assert_eq!(ocr.select([30.0, 25.0, 0.0, 10.0]).text, "l");
        assert_eq!(
            ocr.select([60.0, 25.0, 20.0, 10.0]),
            TextSelection::default()
        );
    }

    #[test]
    fn selected_words_keep_source_order_line_breaks_and_language_spacing() {
        let ocr = recognition(&[
            ("你", [10.0, 10.0, 20.0, 20.0]),
            ("好", [30.0, 11.0, 20.0, 19.0]),
            ("Hello", [60.0, 12.0, 50.0, 17.0]),
            ("world", [120.0, 12.0, 50.0, 22.0]),
            ("第二行", [10.0, 50.0, 60.0, 20.0]),
        ]);
        let all = ocr.select_all();
        assert_eq!(all.text, "你好 Hello world\n第二行");
        assert_eq!(all.bounds.len(), 5);
        assert_eq!(ocr.select([0.0, 0.0, 200.0, 100.0]), all);
        assert_eq!(ocr.select([10.0, 12.0, 40.0, 50.0]).text, "你好\n第二");
    }

    #[test]
    fn punctuation_does_not_add_spaces_inside_chinese_or_before_english_commas() {
        let words = ["你", "好", "，", "世界", "Hello", ",", "world", "!"];
        let boxes: Vec<_> = words
            .iter()
            .enumerate()
            .map(|(i, text)| (*text, [i as f32 * 50.0, 10.0, 40.0, 20.0]))
            .collect();
        assert_eq!(
            recognition(&boxes).select_all().text,
            "你好，世界 Hello, world!"
        );
    }

    #[test]
    fn invalid_boxes_are_ignored_and_legacy_text_can_still_be_selected_all() {
        let mut ocr = recognition(&[
            ("", [0.0, 0.0, 10.0, 10.0]),
            ("zero", [0.0, 0.0, 0.0, 10.0]),
            ("bad", [f32::NAN, 0.0, 10.0, 10.0]),
        ]);
        ocr.text = "legacy OCR text".into();
        assert_eq!(
            ocr.select([0.0, 0.0, 100.0, 100.0]),
            TextSelection::default()
        );
        assert_eq!(
            ocr.select([f32::NAN, 0.0, 10.0, 10.0]),
            TextSelection::default()
        );
        assert_eq!(ocr.select_all().text, "legacy OCR text");
        assert!(ocr.select_all().bounds.is_empty());
    }
}
