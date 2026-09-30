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

pub fn normalize(text: &str) -> String {
    let cjk = |c: char| ('\u{3400}'..='\u{9fff}').contains(&c);
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            if *c == ' ' && i > 0 && i + 1 < chars.len() && cjk(chars[i - 1]) && cjk(chars[i + 1]) {
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
