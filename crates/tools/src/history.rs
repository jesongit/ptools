use image::{RgbaImage, imageops::FilterType};
use ptools_core::{Result, now, read_json, write_json};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct HistoryItem {
    pub id: String,
    pub created: u64,
    pub source: String,
    pub width: u32,
    pub height: u32,
}

pub struct History {
    root: PathBuf,
    days: u32,
}

impl History {
    pub fn new(root: &Path, days: u32) -> Result<Self> {
        let root = root.join("images");
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            days: days.clamp(1, 3650),
        })
    }
    fn valid(id: &str) -> bool {
        !id.is_empty() && id.len() < 80 && id.bytes().all(|b| b.is_ascii_digit() || b == b'-')
    }
    pub fn list(&self) -> Result<Vec<HistoryItem>> {
        let mut items: Vec<HistoryItem> = read_json(&self.root.join("index.json"))?;
        let before = items.len();
        let cutoff = now().saturating_sub(self.days as u64 * 86400);
        items.retain(|item| {
            if !Self::valid(&item.id) {
                return false;
            }
            if item.created < cutoff {
                let _ = fs::remove_file(self.root.join(format!("{}.png", item.id)));
                let _ = fs::remove_file(self.root.join(format!("{}-thumb.png", item.id)));
                false
            } else {
                self.root.join(format!("{}.png", item.id)).is_file()
            }
        });
        if items.len() != before {
            write_json(&self.root.join("index.json"), &items)?;
        }
        items.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
        Ok(items)
    }
    pub fn add(&self, image: &RgbaImage, source: &str) -> Result<HistoryItem> {
        let id = format!(
            "{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos(),
            std::process::id()
        );
        let item = HistoryItem {
            id,
            created: now(),
            source: source.to_owned(),
            width: image.width(),
            height: image.height(),
        };
        let mut items = self.list()?;
        image
            .save(self.root.join(format!("{}.png", item.id)))
            .map_err(|e| e.to_string())?;
        image::DynamicImage::ImageRgba8(image.clone())
            .resize(240, 160, FilterType::Triangle)
            .save(self.root.join(format!("{}-thumb.png", item.id)))
            .map_err(|e| e.to_string())?;
        items.insert(0, item.clone());
        write_json(&self.root.join("index.json"), &items)?;
        Ok(item)
    }
    pub fn load(&self, id: &str, thumbnail: bool) -> Result<RgbaImage> {
        if !Self::valid(id) {
            return Err("图片历史已过期或不存在".into());
        }
        self.load_item(
            self.list()?
                .iter()
                .find(|item| item.id == id)
                .ok_or("图片已过期")?,
            thumbnail,
        )
    }
    pub fn load_item(&self, item: &HistoryItem, thumbnail: bool) -> Result<RgbaImage> {
        if !Self::valid(&item.id) || item.created < now().saturating_sub(self.days as u64 * 86400) {
            return Err("图片已过期".into());
        }
        let id = &item.id;
        image::open(
            self.root
                .join(format!("{id}{}.png", if thumbnail { "-thumb" } else { "" })),
        )
        .map(|i| i.to_rgba8())
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_prunes_and_rejects_paths_outside_cache() {
        let root = std::env::temp_dir().join(format!(
            "ptools-history-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let image = RgbaImage::from_pixel(20, 10, image::Rgba([1, 2, 3, 255]));
        let store = History::new(&root, 7).unwrap();
        let item = store.add(&image, "截图").unwrap();
        let reopened = History::new(&root, 7).unwrap();
        assert_eq!(reopened.load(&item.id, false).unwrap(), image);
        assert!(reopened.load("../../outside", false).is_err());
        let mut items = reopened.list().unwrap();
        items[0].created = now() - 8 * 86400;
        write_json(&reopened.root.join("index.json"), &items).unwrap();
        assert!(reopened.list().unwrap().is_empty());
        assert!(!reopened.root.join(format!("{}.png", item.id)).exists());
        // The only recursive removal is the exact unique fixture created above.
        fs::remove_dir_all(root).unwrap();
    }
}
